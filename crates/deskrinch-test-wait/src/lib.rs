//! How deskrinch's tests wait for something that must happen.
//!
//! A bound on such a wait exists only so that a hung test fails with a
//! message instead of hanging the run; it must never decide a pass. So
//! there is one: [`PATIENCE`], far beyond what anything takes on a loaded
//! machine, paid in full only by a test that would otherwise hang.
//!
//! - [`Deadline`]: a point to give up at, carrying the wait it bounds.
//! - [`wait_for`], [`eventually`] and [`comes_true`]: polling, every
//!   [`POLL`], where nothing signals the change.
//! - [`recv`]: the next value on a channel, where something does.
//!
//! When the patience runs out each panics with what was waited for and,
//! where there is one, the last state seen, at the caller's line
//! ([`#[track_caller]`](https://doc.rust-lang.org/reference/attributes/codegen.html#the-track_caller-attribute)):
//!
//! ```text
//! waited 120s for the window on HEADLESS-2 alone; last seen: [Window { … }]
//! ```
//!
//! A wait for something that must *not* happen (a quiet window) is a
//! different fact, and has no home here.

use std::fmt::Debug;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// How long any test waits for something that must happen: two minutes.
///
/// A hang detector, not a performance expectation: long enough that only a
/// hang reaches it. The case that sets its length is the slowest thing a
/// test waits for that does happen: a component's first frame from a cold
/// GPU start on a loaded machine, whose device and pipeline set-up alone
/// can be silent for many seconds. How fast anything is, is measured with
/// `cargo xtask measure`, never by a test's wait. A hung test costs this
/// much; nothing else does.
pub const PATIENCE: Duration = Duration::from_secs(120);

/// How often a poll looks again, where nothing signals the change.
pub const POLL: Duration = Duration::from_millis(10);

/// A point in time to give up at, and how long the wait it bounds is.
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    at: Instant,
    patience: Duration,
}

impl Deadline {
    /// A deadline [`PATIENCE`] from now.
    #[must_use]
    pub fn patience() -> Self {
        Self::after(PATIENCE)
    }

    /// A deadline `patience` from now: for a bound that is not the
    /// patience, which says why where it is written.
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

    /// Whether it has passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.left().is_zero()
    }

    /// The whole wait it bounds.
    #[must_use]
    pub fn bound(&self) -> Duration {
        self.patience
    }
}

/// What a wait that ran out says: what it waited for, and the last state
/// seen.
#[must_use]
pub fn waited_in_vain(what: &str, last: &impl Debug) -> String {
    format!("waited {PATIENCE:?} for {what}; last seen: {last:#?}")
}

/// Polls `probe` until it gives a value, at most [`PATIENCE`]; `Err` carries
/// the state seen so far, which the panic shows.
///
/// # Panics
///
/// When the patience runs out first, with [`waited_in_vain`]'s message.
#[track_caller]
pub fn wait_for<T, S: Debug>(what: &str, mut probe: impl FnMut() -> Result<T, S>) -> T {
    let deadline = Deadline::patience();
    loop {
        match probe() {
            Ok(value) => return value,
            Err(last) if deadline.passed() => panic!("{}", waited_in_vain(what, &last)),
            Err(_) => thread::sleep(POLL),
        }
    }
}

/// Polls `check` until it holds, at most [`PATIENCE`].
///
/// # Panics
///
/// When the patience runs out first, naming `what`.
#[track_caller]
pub fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Deadline::patience();
    while !check() {
        assert!(!deadline.passed(), "waited {PATIENCE:?} for {what}");
        thread::sleep(POLL);
    }
}

/// Whether `check` comes to hold within [`PATIENCE`], polled; for a test
/// that must end what it started before it asserts, or that says more than
/// [`eventually`] can when it fails.
#[must_use]
pub fn comes_true(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Deadline::patience();
    loop {
        if check() {
            return true;
        }
        if deadline.passed() {
            return false;
        }
        thread::sleep(POLL);
    }
}

/// The next value on `values`, waited for at most [`PATIENCE`].
///
/// # Panics
///
/// When nothing arrives within the patience, or when every sender is gone
/// (nothing more can arrive), naming `what`; the two say which.
#[must_use]
#[track_caller]
pub fn recv<T>(what: &str, values: &Receiver<T>) -> T {
    match values.recv_timeout(PATIENCE) {
        Ok(value) => value,
        Err(RecvTimeoutError::Timeout) => {
            panic!("waited {PATIENCE:?} for {what}: nothing arrived")
        }
        Err(RecvTimeoutError::Disconnected) => {
            panic!("waiting for {what}: the sender is gone, so nothing more can arrive")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn a_deadline_knows_its_bound_and_what_is_left() {
        let deadline = Deadline::after(Duration::from_secs(5));
        assert_eq!(deadline.bound(), Duration::from_secs(5));
        assert!(deadline.left() <= Duration::from_secs(5));
        assert!(!deadline.passed());
        assert!(Deadline::after(Duration::ZERO).passed());
        assert_eq!(Deadline::patience().bound(), PATIENCE);
    }

    #[test]
    fn waits_return_as_soon_as_they_can() {
        let mut tries = 0;
        let value = wait_for("the third try", || {
            tries += 1;
            if tries == 3 { Ok(tries) } else { Err(tries) }
        });
        assert_eq!(value, 3);
        eventually("true", || true);
        assert!(comes_true(|| true));
        let (sender, values) = mpsc::channel();
        sender.send(7).unwrap();
        assert_eq!(recv("seven", &values), 7);
    }

    #[test]
    fn a_wait_in_vain_names_what_and_the_last_state() {
        let message = waited_in_vain("the window", &vec!["HEADLESS-1"]);
        assert!(
            message.starts_with("waited 120s for the window; last seen: ["),
            "{message}"
        );
        assert!(message.contains("\"HEADLESS-1\""), "{message}");
    }

    #[test]
    #[should_panic(expected = "the sender is gone")]
    fn a_closed_channel_is_told_from_a_slow_one() {
        let (sender, values) = mpsc::channel::<()>();
        drop(sender);
        let () = recv("nothing", &values);
    }
}
