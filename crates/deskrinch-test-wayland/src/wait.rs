//! Waiting for a compositor, never for longer than a deadline.
//!
//! `EventQueue::blocking_dispatch` waits for the compositor as long as it
//! takes; a compositor that never answers hangs the test. These read the
//! socket only when [`poll`](rustix::event::poll) says there is something
//! to read, and only until the deadline.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::backend::protocol::Message;
use wayland_client::backend::{Backend, ObjectData, ObjectId, WaylandError};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_display;
use wayland_client::{Connection, DispatchError, EventQueue, Proxy};

/// Why a wait ended without what was waited for.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WaitError {
    /// The deadline passed first.
    #[error("the compositor did not answer within {0:?}")]
    TimedOut(Duration),
    /// Dispatching an event failed (a protocol error, or the connection
    /// closed).
    #[error("the connection to the compositor failed")]
    Dispatch(#[source] DispatchError),
    /// Sending to or reading from the compositor failed.
    #[error("the connection to the compositor failed")]
    Connection(#[source] WaylandError),
    /// The connection is closed, so nothing can be sent.
    #[error("the connection to the compositor is closed")]
    Closed,
    /// Waiting on the socket failed.
    #[error("could not wait on the compositor's socket")]
    Poll(#[source] std::io::Error),
}

/// A point in time to give up at, and how long the wait it bounds is.
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    at: Instant,
    patience: Duration,
}

impl Deadline {
    /// A deadline `patience` from now.
    #[must_use]
    pub fn after(patience: Duration) -> Self {
        Self {
            at: Instant::now() + patience,
            patience,
        }
    }

    /// How long is left; zero once it has passed.
    #[must_use]
    pub fn left(&self) -> Duration {
        self.at.saturating_duration_since(Instant::now())
    }

    /// The whole wait it bounds.
    #[must_use]
    pub fn patience(&self) -> Duration {
        self.patience
    }

    /// The error for this deadline having passed.
    #[must_use]
    pub fn timed_out(&self) -> WaitError {
        WaitError::TimedOut(self.patience)
    }
}

/// Dispatches `queue`'s events into `state` until `done` takes the state,
/// or `deadline` passes.
///
/// # Errors
///
/// [`WaitError::TimedOut`] when the deadline passes first; otherwise when
/// the connection fails.
pub fn dispatch_until<S>(
    queue: &mut EventQueue<S>,
    state: &mut S,
    deadline: Deadline,
    done: impl Fn(&S) -> bool,
) -> Result<(), WaitError> {
    loop {
        queue.dispatch_pending(state).map_err(WaitError::Dispatch)?;
        if done(state) {
            return Ok(());
        }
        queue.flush().map_err(WaitError::Connection)?;
        // `None`: events arrived meanwhile; dispatch them first.
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let left = deadline.left();
        if left.is_zero() {
            return Err(deadline.timed_out());
        }
        let readable = {
            let fd = guard.connection_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            let timeout = Timespec::try_from(left).unwrap_or(Timespec {
                tv_sec: i64::MAX,
                tv_nsec: 0,
            });
            match poll(&mut fds, Some(&timeout)) {
                Ok(ready) => ready > 0,
                Err(rustix::io::Errno::INTR) => false,
                Err(errno) => return Err(WaitError::Poll(errno.into())),
            }
        };
        if readable {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(WaitError::Connection(error)),
            }
        }
        // Not readable: the guard is dropped, cancelling the read, and the
        // loop looks at the deadline again.
    }
}

/// [`dispatch_until`] for a round trip: returns once the compositor has
/// handled every request sent before it, and the events they caused have
/// been dispatched.
///
/// # Errors
///
/// As [`dispatch_until`].
pub fn roundtrip<S>(
    connection: &Connection,
    queue: &mut EventQueue<S>,
    state: &mut S,
    deadline: Deadline,
) -> Result<(), WaitError> {
    let synced = Arc::new(Synced::default());
    let data: Arc<dyn ObjectData> = Arc::<Synced>::clone(&synced);
    connection
        .display()
        .send_constructor::<WlCallback>(wl_display::Request::Sync {}, data)
        .map_err(|_| WaitError::Closed)?;
    dispatch_until(queue, state, deadline, |_| {
        synced.done.load(Ordering::Acquire)
    })
}

/// The `wl_callback` of a [`roundtrip`]: done when the compositor answers.
#[derive(Debug, Default)]
struct Synced {
    done: AtomicBool,
}

impl ObjectData for Synced {
    fn event(
        self: Arc<Self>,
        _: &Backend,
        _: Message<ObjectId, OwnedFd>,
    ) -> Option<Arc<dyn ObjectData>> {
        // `wl_callback.done`, its one event.
        self.done.store(true, Ordering::Release);
        None
    }

    fn destroyed(&self, _: ObjectId) {}
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;

    use super::*;

    /// A client whose compositor end never answers.
    fn silent() -> (Connection, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        (Connection::from_socket(ours).unwrap(), theirs)
    }

    /// A client with nothing to dispatch.
    #[derive(Debug, Default)]
    struct Nothing;

    #[test]
    fn a_roundtrip_nobody_answers_times_out() {
        let (connection, _compositor) = silent();
        let mut queue = connection.new_event_queue::<Nothing>();
        let error = roundtrip(
            &connection,
            &mut queue,
            &mut Nothing,
            Deadline::after(Duration::from_millis(200)),
        )
        .unwrap_err();
        assert!(
            matches!(error, WaitError::TimedOut(patience) if patience == Duration::from_millis(200)),
            "{error:?}"
        );
    }

    #[test]
    fn a_wait_already_done_returns_at_once() {
        let (connection, _compositor) = silent();
        let mut queue = connection.new_event_queue::<Nothing>();
        dispatch_until(
            &mut queue,
            &mut Nothing,
            Deadline::after(Duration::ZERO),
            |_| true,
        )
        .unwrap();
    }

    #[test]
    fn a_compositor_that_hangs_up_is_a_failure_not_a_timeout() {
        let (connection, compositor) = silent();
        drop(compositor);
        let mut queue = connection.new_event_queue::<Nothing>();
        let error = roundtrip(
            &connection,
            &mut queue,
            &mut Nothing,
            Deadline::after(Duration::from_secs(600)),
        )
        .unwrap_err();
        assert!(!matches!(error, WaitError::TimedOut(_)), "{error:?}");
    }
}
