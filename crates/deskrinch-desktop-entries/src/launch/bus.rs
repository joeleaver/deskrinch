//! The session-bus plumbing shared by systemd scopes and D-Bus activation.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Poll, Waker};
use std::thread;
use std::time::Duration;

use zbus::Connection;

/// Connects to the bus at `address`, with `method_timeout` on every call.
pub(super) async fn connect(address: &str, method_timeout: Duration) -> zbus::Result<Connection> {
    zbus::connection::Builder::address(address)
        .method_timeout(method_timeout)
        .build()
        .await
}

/// Whether some connection owns `name` on the bus.
pub(super) async fn name_has_owner(connection: &Connection, name: &str) -> zbus::Result<bool> {
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &name,
        )
        .await?;
    reply.body().deserialize()
}

/// Runs `future` until it completes or `limit` passes, whichever is first.
pub(super) async fn with_timeout<F: Future>(future: F, limit: Duration) -> Option<F::Output> {
    let deadline = Deadline::start(limit);
    let mut future = pin!(future);
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(output));
        }
        if deadline.passed(cx.waker()) {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    })
    .await
}

/// A timer on a thread of its own, which ends early when the deadline is
/// dropped. zbus's runtime keeps its timers private, and this crate has no
/// runtime of its own.
struct Deadline {
    shared: Arc<(Mutex<DeadlineState>, Condvar)>,
}

#[derive(Default)]
struct DeadlineState {
    passed: bool,
    cancelled: bool,
    waker: Option<Waker>,
}

impl Deadline {
    fn start(limit: Duration) -> Self {
        let shared = Arc::new((Mutex::new(DeadlineState::default()), Condvar::new()));
        let timer = Arc::clone(&shared);
        // If the thread cannot start, the deadline never passes and the
        // call is bounded only by the connection's method timeout.
        let _detached = thread::Builder::new()
            .name("desktop-entries-deadline".into())
            .spawn(move || {
                let (lock, condvar) = &*timer;
                let state = lock.lock().unwrap_or_else(PoisonError::into_inner);
                let (mut state, _) = condvar
                    .wait_timeout_while(state, limit, |state| !state.cancelled)
                    .unwrap_or_else(PoisonError::into_inner);
                if !state.cancelled {
                    state.passed = true;
                    if let Some(waker) = state.waker.take() {
                        waker.wake();
                    }
                }
            });
        Self { shared }
    }

    /// Whether the deadline has passed; if not, `waker` is woken when it
    /// does.
    fn passed(&self, waker: &Waker) -> bool {
        let mut state = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.passed {
            state.waker = Some(waker.clone());
        }
        state.passed
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        let (lock, condvar) = &*self.shared;
        lock.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancelled = true;
        condvar.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;

    #[test]
    fn a_timeout_ends_a_pending_future() {
        let result = zbus::block_on(with_timeout(pending::<()>(), Duration::from_millis(20)));
        assert_eq!(result, None);
    }

    #[test]
    fn a_ready_future_beats_the_timeout() {
        let result = zbus::block_on(with_timeout(async { 7 }, Duration::from_secs(60)));
        assert_eq!(result, Some(7));
    }
}
