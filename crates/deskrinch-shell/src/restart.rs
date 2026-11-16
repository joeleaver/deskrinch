//! A component's restart policy ([`Restart`]), and the rule that follows
//! it ([`Starts`]): one home for both, read by every starter, the nested
//! desktop now and the login session's supervisor later.
//!
//! The policy is systemd's, so that a unit can state it without
//! interpretation ([`Restart::systemd_lines`]):
//!
//! - **What a failure is** ([`Exit::of`]): systemd's for `Restart=on-failure`.
//!   An exit with code 0, or an end by `SIGHUP`, `SIGINT`, `SIGTERM` or
//!   `SIGPIPE`, is clean and is not restarted; any other exit code, or any
//!   other signal, is a failure. So a component ended with a plain `kill`
//!   (`SIGTERM`) stays gone.
//! - **What is never restarted** ([`Exit::CannotRunHere`]): the status
//!   [`CANNOT_RUN_HERE`], which a component ends with when it cannot run
//!   where it was started. systemd's `RestartPreventExitStatus=`.
//! - **What the limit counts** ([`Starts`]): starts, the first included, in
//!   systemd's start-limit window (`StartLimitBurst=`,
//!   `StartLimitIntervalSec=`), which is not a sliding one. The rule is
//!   systemd's `ratelimit_below` (`src/basic/ratelimit.c`, systemd 259):
//!   a start more than `interval` after the window began begins a new
//!   window, counted as its first start; any other start adds one to the
//!   count, and is allowed while the count is at most `burst`.
//! - **Not part of it:** the pause before a restart. A unit waits its
//!   `RestartSec=` (systemd's default is 100 ms); a starter that restarts at
//!   once follows the same policy.
//!
//! No I/O here and no clock: the times are the caller's.

use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use crate::exit_status::CANNOT_RUN_HERE;

/// Whether, and how often, a component that ends is started again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    /// Never started again (`Restart=no`).
    Never,
    /// Started again after a failure ([`Exit::Failed`], `Restart=on-failure`),
    /// unless the start would be more than `burst` starts in the window of
    /// `interval` ([`Starts`]; `StartLimitBurst=`,
    /// `StartLimitIntervalSec=`).
    OnFailure {
        /// The most starts in a window, the first start included.
        burst: u32,
        /// How long a window lasts from its first start.
        interval: Duration,
    },
}

impl Restart {
    /// The policy as a systemd unit states it, one line each: `Restart=` and
    /// `RestartPreventExitStatus=` in `[Service]`, the start limit in
    /// `[Unit]`.
    #[must_use]
    pub fn systemd_lines(self) -> Vec<String> {
        match self {
            Self::Never => vec!["Restart=no".to_owned()],
            Self::OnFailure { burst, interval } => vec![
                "Restart=on-failure".to_owned(),
                format!("RestartPreventExitStatus={CANNOT_RUN_HERE}"),
                format!("StartLimitBurst={burst}"),
                format!("StartLimitIntervalSec={}", interval.as_secs()),
            ],
        }
    }
}

/// How a component ended, as the policy reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Code 0, or ended by `SIGHUP`, `SIGINT`, `SIGTERM` or `SIGPIPE`.
    Clean,
    /// Code [`CANNOT_RUN_HERE`]: it cannot run where it was started.
    CannotRunHere,
    /// Any other code, or any other signal.
    Failed,
}

/// The signals systemd counts as a clean end for `Restart=on-failure`.
const CLEAN_SIGNALS: [i32; 4] = [libc::SIGHUP, libc::SIGINT, libc::SIGTERM, libc::SIGPIPE];

impl Exit {
    /// How `status` reads under systemd's rule for `Restart=on-failure`,
    /// with [`CANNOT_RUN_HERE`] in `RestartPreventExitStatus=`.
    #[must_use]
    pub fn of(status: &ExitStatus) -> Self {
        match (status.code(), status.signal()) {
            (Some(0), _) => Self::Clean,
            (Some(code), _) if code == i32::from(CANNOT_RUN_HERE) => Self::CannotRunHere,
            (None, Some(signal)) if CLEAN_SIGNALS.contains(&signal) && !status.core_dumped() => {
                Self::Clean
            }
            _ => Self::Failed,
        }
    }
}

/// What to do about a component that ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Start it again: this is the `start`-th start of the window.
    Restart {
        /// Its number in the window, from 1.
        start: u32,
    },
    /// It ended cleanly: not started again.
    Clean,
    /// It cannot run where it was started ([`Exit::CannotRunHere`]): not
    /// started again.
    CannotRunHere,
    /// Its policy is [`Restart::Never`]: not started again.
    Never,
    /// It failed, and starting it again would pass the limit: not started
    /// again, ever (systemd leaves such a unit failed).
    Exhausted {
        /// The limit's starts.
        burst: u32,
        /// The limit's window.
        interval: Duration,
    },
}

/// A component's starts in systemd's start-limit window, and the verdict on
/// each end.
#[derive(Debug, Clone)]
pub struct Starts {
    policy: Restart,
    /// When the window began.
    begin: Instant,
    /// The starts in the window, the first included.
    count: u32,
    exhausted: bool,
}

impl Starts {
    /// The starts of a component under `policy`, first started at `first`.
    #[must_use]
    pub fn new(policy: Restart, first: Instant) -> Self {
        Self {
            policy,
            begin: first,
            count: 1,
            exhausted: false,
        }
    }

    /// The verdict on the component's ending as `exit` says, at `now`; a
    /// [`Verdict::Restart`] counts the start it asks for, at `now`. Once
    /// exhausted, it stays so.
    pub fn after_exit(&mut self, exit: Exit, now: Instant) -> Verdict {
        let Restart::OnFailure { burst, interval } = self.policy else {
            return Verdict::Never;
        };
        if self.exhausted {
            return Verdict::Exhausted { burst, interval };
        }
        match exit {
            Exit::Clean => return Verdict::Clean,
            Exit::CannotRunHere => return Verdict::CannotRunHere,
            Exit::Failed => {}
        }
        // systemd's `ratelimit_below`: past the window, a new one.
        if now.saturating_duration_since(self.begin) > interval {
            self.begin = now;
            self.count = 1;
            return Verdict::Restart { start: 1 };
        }
        self.count = self.count.saturating_add(1);
        if self.count <= burst {
            Verdict::Restart { start: self.count }
        } else {
            self.exhausted = true;
            Verdict::Exhausted { burst, interval }
        }
    }
}

#[cfg(test)]
mod tests;
