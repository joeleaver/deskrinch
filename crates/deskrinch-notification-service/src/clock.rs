//! The time source expiry runs on, replaceable so expiry is tested without
//! sleeping.

use std::fmt;
use std::sync::{Mutex, PoisonError};
use std::task::Waker;
use std::time::{Duration, Instant};

/// A monotonic clock the service reads to time notifications out.
///
/// [`SystemClock`] is the real one. [`ManualClock`] stands still until it is
/// advanced, for tests. Another implementation must keep `now` monotonic and
/// keep its two wake-up methods consistent with it, as the method docs say.
pub trait Clock: Send + Sync + 'static {
    /// The current time on this clock.
    fn now(&self) -> Instant;

    /// How long, in real time, the service should wait before it reads
    /// [`now`](Self::now) again to see whether `deadline` has passed.
    ///
    /// `None` means the clock does not advance on its own: the service then
    /// waits until a waker given to [`wake_on_advance`](Self::wake_on_advance)
    /// is woken.
    fn real_time_until(&self, deadline: Instant) -> Option<Duration>;

    /// Registers `waker` to be woken whenever this clock jumps forward other
    /// than with real time. A clock that only follows real time ignores it.
    fn wake_on_advance(&self, waker: Waker);
}

/// The system's monotonic clock, [`Instant::now`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn real_time_until(&self, deadline: Instant) -> Option<Duration> {
        Some(deadline.saturating_duration_since(Instant::now()))
    }

    fn wake_on_advance(&self, _waker: Waker) {}
}

/// A clock that moves only when [`advance`](Self::advance) is called.
///
/// Advancing wakes every service that runs on it, so expiry happens as soon
/// as the clock passes a deadline, with no real waiting.
pub struct ManualClock {
    start: Instant,
    elapsed: Mutex<Duration>,
    wakers: Mutex<Vec<Waker>>,
}

impl ManualClock {
    /// A clock standing at the moment it is created.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
            wakers: Mutex::new(Vec::new()),
        }
    }

    /// Moves the clock forward by `by` and wakes the services on it.
    pub fn advance(&self, by: Duration) {
        {
            let mut elapsed = self.elapsed.lock().unwrap_or_else(PoisonError::into_inner);
            *elapsed = elapsed.saturating_add(by);
        }
        let wakers = self.wakers.lock().unwrap_or_else(PoisonError::into_inner);
        for waker in wakers.iter() {
            waker.wake_by_ref();
        }
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for ManualClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let elapsed = *self.elapsed.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("ManualClock")
            .field("elapsed", &elapsed)
            .finish_non_exhaustive()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + *self.elapsed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn real_time_until(&self, _deadline: Instant) -> Option<Duration> {
        None
    }

    fn wake_on_advance(&self, waker: Waker) {
        let mut wakers = self.wakers.lock().unwrap_or_else(PoisonError::into_inner);
        wakers.retain(|w| !w.will_wake(&waker));
        wakers.push(waker);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;

    use super::*;

    struct Count(AtomicUsize);

    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn manual_clock_moves_only_when_advanced_and_wakes() {
        let clock = ManualClock::new();
        let count = Arc::new(Count(AtomicUsize::new(0)));
        clock.wake_on_advance(Waker::from(Arc::clone(&count)));
        let t0 = clock.now();
        assert_eq!(clock.now(), t0);
        clock.advance(Duration::from_secs(3));
        assert_eq!(clock.now(), t0 + Duration::from_secs(3));
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(clock.real_time_until(t0), None);
    }

    #[test]
    fn system_clock_waits_until_the_deadline() {
        let clock = SystemClock;
        let past = clock.now();
        assert_eq!(clock.real_time_until(past), Some(Duration::ZERO));
        let wait = clock.real_time_until(clock.now() + Duration::from_secs(60));
        assert!(wait.is_some_and(|w| w > Duration::from_secs(59)));
    }
}
