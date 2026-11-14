//! A timer on the wall clock that also reports the wall clock being set.
//!
//! It is a Linux `timerfd` on `CLOCK_REALTIME`, armed with an absolute
//! expiry (`TFD_TIMER_ABSTIME`) and `TFD_TIMER_CANCEL_ON_SET`:
//!
//! - **Absolute on the wall clock.** The timer expires when the wall clock
//!   reaches the boundary, however the clock got there. A relative sleep, or
//!   any timer on `CLOCK_MONOTONIC`, measures elapsed time instead: it does
//!   not advance during suspend, and it ignores a change of the wall clock,
//!   so it wakes late or early by exactly the amount the clock moved.
//! - **Cancelled on set.** When the wall clock changes discontinuously
//!   (`clock_settime` or `settimeofday` from `date -s` or
//!   `timedatectl set-time`, an NTP step through `adjtimex(ADJ_SETOFFSET)`,
//!   the kernel inserting or deleting a leap second, or the clock catching
//!   up on resume from suspend), the kernel cancels the timer and a read
//!   fails with `ECANCELED`. The clock then reads the time anew, at once,
//!   instead of when the old expiry comes round. A gradual NTP slew is not a
//!   discontinuity and needs nothing: the absolute expiry tracks it.
//!
//! A change of the clock in the moment between computing an expiry and
//! arming the timer cancels nothing, so the worker reads the clock after
//! arming, not before (see [`crate::clock`]).

use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use jiff::Timestamp;
use rustix::io::Errno;
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create,
    timerfd_settime,
};

/// What reading the timer found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fired {
    /// The expiry passed.
    Expired,
    /// The wall clock was set; the timer is disarmed.
    ClockSet,
    /// Nothing yet.
    Pending,
}

/// A one-shot timer at an instant of the wall clock.
#[derive(Debug)]
pub(crate) struct WallTimer {
    fd: OwnedFd,
}

impl WallTimer {
    pub(crate) fn new() -> io::Result<Self> {
        let fd = timerfd_create(
            TimerfdClockId::Realtime,
            TimerfdFlags::CLOEXEC | TimerfdFlags::NONBLOCK,
        )?;
        Ok(Self { fd })
    }

    /// Arms the timer to expire when the wall clock reaches `at`. Returns
    /// [`Fired::ClockSet`] when the kernel reports the clock was set since
    /// the timer was last read.
    pub(crate) fn arm(&self, at: Timestamp) -> io::Result<Fired> {
        let expiry = Itimerspec {
            it_interval: Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: Timespec {
                tv_sec: at.as_second(),
                tv_nsec: at.subsec_nanosecond().into(),
            },
        };
        let flags = TimerfdTimerFlags::ABSTIME | TimerfdTimerFlags::CANCEL_ON_SET;
        match timerfd_settime(&self.fd, flags, &expiry) {
            Ok(_) => Ok(Fired::Pending),
            Err(Errno::CANCELED) => Ok(Fired::ClockSet),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads what happened since the timer was armed, without blocking.
    pub(crate) fn read(&self) -> io::Result<Fired> {
        let mut expirations = [0_u8; 8];
        match rustix::io::read(&self.fd, &mut expirations) {
            Ok(_) => Ok(Fired::Expired),
            Err(Errno::CANCELED) => Ok(Fired::ClockSet),
            Err(Errno::AGAIN) => Ok(Fired::Pending),
            Err(error) => Err(error.into()),
        }
    }
}

impl AsFd for WallTimer {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::SignedDuration;

    use crate::test_patience::PATIENCE;

    /// Waits until the kernel says `timer` has fired, as the clock's worker
    /// does, within [`PATIENCE`].
    fn wait_fired(timer: &WallTimer) {
        let mut fds = [rustix::event::PollFd::new(
            timer,
            rustix::event::PollFlags::IN,
        )];
        let patience = Timespec {
            tv_sec: PATIENCE.as_secs().try_into().unwrap(),
            tv_nsec: 0,
        };
        assert_eq!(
            rustix::event::poll(&mut fds, Some(&patience)).unwrap(),
            1,
            "the timer did not fire within {PATIENCE:?}"
        );
    }

    #[test]
    fn expires_at_a_wall_clock_instant_and_not_before() {
        let timer = WallTimer::new().unwrap();
        assert_eq!(timer.read().unwrap(), Fired::Pending);
        // An hour ahead: pending, however slow the machine.
        let later = Timestamp::now() + SignedDuration::from_hours(1);
        assert_eq!(timer.arm(later).unwrap(), Fired::Pending);
        assert_eq!(timer.read().unwrap(), Fired::Pending);
        // A moment ahead: it fires, and not before its instant.
        let at = Timestamp::now() + SignedDuration::from_millis(50);
        assert_eq!(timer.arm(at).unwrap(), Fired::Pending);
        wait_fired(&timer);
        assert!(Timestamp::now() >= at, "fired before {at}");
        assert_eq!(timer.read().unwrap(), Fired::Expired);
    }

    #[test]
    fn an_expiry_in_the_past_has_expired() {
        let timer = WallTimer::new().unwrap();
        timer
            .arm(Timestamp::now() - SignedDuration::from_secs(1))
            .unwrap();
        // The kernel fires it from its timer interrupt, a moment later.
        wait_fired(&timer);
        assert_eq!(timer.read().unwrap(), Fired::Expired);
    }
}
