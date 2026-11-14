//! [`Synced`]: a `wl_display.sync`'s answer, which the client's thread
//! signals and a caller waits for.
//!
//! The compositor answers a `sync` only once it has processed every
//! request the client sent before it: once a window's first commit is
//! answered this way, the compositor has mapped the window and told the
//! clients that list windows about it.

use std::sync::{Arc, Condvar, Mutex};

use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::{Connection, Dispatch, QueueHandle};

use crate::client::Client;
use crate::{Error, lock};
use deskrinch_test_wait::{Deadline, PATIENCE};

/// Whether a `sync` has been answered.
#[derive(Debug, Default)]
pub(crate) struct Synced {
    done: Mutex<bool>,
    changed: Condvar,
}

impl Synced {
    /// Waits, at most [`PATIENCE`](deskrinch_test_wait::PATIENCE), until the `sync` is answered.
    pub(crate) fn wait(&self, what: &'static str) -> Result<(), Error> {
        let deadline = Deadline::patience();
        let mut done = lock(&self.done);
        while !*done {
            let left = deadline.left();
            if left.is_zero() {
                return Err(Error::TimedOut {
                    what,
                    patience: PATIENCE,
                });
            }
            done = self
                .changed
                .wait_timeout(done, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        Ok(())
    }
}

impl Dispatch<WlCallback, Arc<Synced>> for Client {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        synced: &Arc<Synced>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            *lock(&synced.done) = true;
            synced.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_waiter_hears_the_answer() {
        let synced = Arc::new(Synced::default());
        let answering = Arc::clone(&synced);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            *lock(&answering.done) = true;
            answering.changed.notify_all();
        });
        synced.wait("an answer").unwrap();
    }
}
