//! Sleeps and timeouts for futures under any executor, on one thread per
//! timer.
//!
//! deskrinch drives its D-Bus work with `zbus::block_on`, whose runtime
//! keeps its timers private, and has no async runtime of its own. A
//! [`Timer`] gives such code [`Sleep`] and [`Timeout`] futures: one thread
//! per timer keeps every deadline in a heap and wakes each future when its
//! deadline passes. Deadlines are [`Instant`]s, so a change of the wall
//! clock moves none of them.
//!
//! Dropping a future removes its deadline's waker at once; the thread ends
//! when the last [`Timer`] handle and the last of its futures are gone.
//!
//! # Example
//!
//! With `zbus::block_on`, or any other executor (`block_on` here is a
//! minimal one):
//!
//! ```
//! use std::time::Duration;
//!
//! use deskrinch_timer::{Elapsed, Timer};
//!
//! let timer = Timer::start()?;
//! block_on(timer.sleep(Duration::from_millis(10)));
//! assert_eq!(block_on(timer.timeout(Duration::from_secs(5), async { 7 })), Ok(7));
//! let never = std::future::pending::<()>();
//! assert_eq!(block_on(timer.timeout(Duration::from_millis(10), never)), Err(Elapsed));
//! # fn block_on<F: std::future::Future>(f: F) -> F::Output {
//! #     use std::sync::Arc;
//! #     use std::task::{Context, Poll, Wake};
//! #     struct Unpark(std::thread::Thread);
//! #     impl Wake for Unpark {
//! #         fn wake(self: Arc<Self>) { self.0.unpark(); }
//! #     }
//! #     let waker = Arc::new(Unpark(std::thread::current())).into();
//! #     let mut cx = Context::from_waker(&waker);
//! #     let mut f = std::pin::pin!(f);
//! #     loop {
//! #         if let Poll::Ready(v) = f.as_mut().poll(&mut cx) { return v; }
//! #         std::thread::park();
//! #     }
//! # }
//! # Ok::<(), deskrinch_timer::StartError>(())
//! ```

mod shared;
#[cfg(test)]
mod tests;

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use shared::Shared;

/// A timer: a thread that wakes its futures when their deadlines pass.
/// Cloning it is cheap, and the clones share the thread.
pub struct Timer {
    shared: Arc<Shared>,
}

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer").finish_non_exhaustive()
    }
}

impl Timer {
    /// Starts the timer's thread.
    ///
    /// # Errors
    ///
    /// [`StartError`] when the thread cannot be started.
    pub fn start() -> Result<Self, StartError> {
        Ok(Self {
            shared: Shared::start().map_err(StartError)?,
        })
    }

    /// A future that is ready once `duration` has passed.
    #[must_use]
    pub fn sleep(&self, duration: Duration) -> Sleep {
        self.sleep_until(Instant::now() + duration)
    }

    /// A future that is ready once `deadline` has passed.
    #[must_use]
    pub fn sleep_until(&self, deadline: Instant) -> Sleep {
        let key = self.shared.insert(deadline);
        Sleep {
            shared: Arc::clone(&self.shared),
            key,
            deadline,
        }
    }

    /// `future`, unless `duration` passes first: then [`Elapsed`], and the
    /// future is dropped. A future that is ready when the time is up still
    /// wins.
    pub fn timeout<F: Future>(&self, duration: Duration, future: F) -> Timeout<F> {
        Timeout {
            future: Box::pin(future),
            sleep: self.sleep(duration),
        }
    }
}

impl Clone for Timer {
    fn clone(&self) -> Self {
        self.shared.join();
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.shared.leave();
    }
}

/// Ready once its deadline has passed; made by [`Timer::sleep`].
pub struct Sleep {
    shared: Arc<Shared>,
    key: u64,
    deadline: Instant,
}

impl fmt::Debug for Sleep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sleep")
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl Sleep {
    /// When it is ready.
    #[must_use]
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // Whether the deadline has passed is decided under the timer's lock,
        // with the waker kept in the same step, never from the clock here:
        // the thread may take the deadline at any moment.
        if self.shared.set_waker(self.key, cx.waker()) {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        self.shared.remove(self.key);
    }
}

/// A future with a deadline; made by [`Timer::timeout`].
pub struct Timeout<F> {
    future: Pin<Box<F>>,
    sleep: Sleep,
}

impl<F> fmt::Debug for Timeout<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timeout")
            .field("deadline", &self.sleep.deadline)
            .finish_non_exhaustive()
    }
}

impl<F: Future> Future for Timeout<F> {
    type Output = Result<F::Output, Elapsed>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(output) = self.future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        Pin::new(&mut self.sleep).poll(cx).map(|()| Err(Elapsed))
    }
}

/// The timer's thread could not be started.
#[derive(Debug)]
pub struct StartError(io::Error);

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("could not start the timer's thread")
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// A [`Timeout`]'s time passed before its future was ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the deadline passed")
    }
}

impl std::error::Error for Elapsed {}
