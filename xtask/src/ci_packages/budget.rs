//! How long each `apt-get` run may take: one budget for the whole package
//! step, counted from `ci-packages update`'s start, which the plan carries
//! to `ci-packages install`.
//!
//! `update` and the listing have caps of their own; `install` has none and
//! is given what is left. (Making the cache's directory comes first, with
//! ten seconds of its own, counted in the time used.) Every run's limit is
//! at most what remains of the
//! budget less [`RESERVE`], so the step, its verdict printed and the cache
//! saved, ends before GitHub ends the tightest job, whatever the earlier
//! runs and the restore took: a restore that took its time has left little
//! to fetch, and a miss is known in seconds.

use std::time::{Duration, SystemTime};

use super::bounded::KILL_AFTER;
use super::classify::Step;

/// What the tightest jobs leave the package step before GitHub ends them:
/// the Tests job's 30 minute limit less its other steps at their longest
/// over CI's runs of 2026-11-15 to 18 (cold caches included). The
/// compositor tests' build job and shard jobs, 30 minutes each since
/// 2026-11-23, leave more: a cold build of 8 to 9 minutes, a shard's run of
/// at most about 11.
const TIGHTEST_JOB_REMAINDER: Duration = Duration::from_secs(14 * 60 + 42);

/// The time before `update` starts: building xtask, which runs it. At most
/// 44 s over the 220 job logs of 2026-11-17 and 18 that reached apt (median
/// 9 s).
const BUILD_ALLOWANCE: Duration = Duration::from_mins(1);

/// The step's budget, from `update`'s start.
pub(super) const BUDGET: Duration = TIGHTEST_JOB_REMAINDER.saturating_sub(BUILD_ALLOWANCE);

/// The save, on `main` and only for a key not restored exactly: uploading
/// at most 115 MB, which the action does not bound. The Tests job's build
/// cache, 2.9 GB, took 80 s to pack and send on 2026-11-18: at that rate
/// 115 MB is 3 s.
const SAVE_ALLOWANCE: Duration = Duration::from_secs(30);

/// What the step keeps after a run's limit: the run's grace after
/// `SIGTERM` and the save.
pub(super) const RESERVE: Duration = KILL_AFTER.saturating_add(SAVE_ALLOWANCE);

/// The least `install` is given: about three times the longest install
/// seen with nothing slow to fetch (KWin's 308 packages, 44 s of which 20
/// fetching, on 2026-11-19). With less, it could not finish even from a
/// full cache, and a cut-off would blame the mirror for time spent before
/// it.
pub(super) const INSTALL_FLOOR: Duration = Duration::from_mins(2);

/// A run's own cap. `install` has none: it takes what is left.
///
/// - `update` fetches 4.2 to 5.0 MB of indexes from two hosts (the
///   region's mirror and `security.ubuntu.com`), and is where a server that
///   cannot be reached shows. Over the 49 runs of it on the night of
///   2026-11-19, the slowest that finished took 33 s; one was cut off at
///   two minutes: the mirror's indexes came in 2 s, then
///   `security.ubuntu.com` stalled, its `main` index failing twice (apt's
///   20 s request timeout, at 37 s and 116 s) with its third and last
///   attempt pending, which would have ended by about 195 s. Four minutes
///   lets apt's own retries run out on a stalled host, so that apt ends
///   `update` itself (a warning, the index left as it was) rather than
///   the cap; it is also what a name that never resolves costs, since apt
///   has no timeout for that. With `update` taking all of it, the install
///   is still left about eight minutes: past its floor, and room for what
///   a cached job fetched that night (13.6 MB, the seven files one runner
///   image needed more than the other, in 140 to 170 s).
/// - The listing reads what `update` fetched and fetches nothing, in well
///   under a second.
fn cap(step: Step) -> Option<Duration> {
    match step {
        Step::Update => Some(Duration::from_mins(4)),
        Step::List => Some(Duration::from_secs(20)),
        Step::Install => None,
    }
}

/// The least a run is given: a capped run its whole cap (shortened, it
/// would be cut off for want of the step's time, not its own), `install`
/// [`INSTALL_FLOOR`].
fn floor(step: Step) -> Duration {
    cap(step).unwrap_or(INSTALL_FLOOR)
}

/// How long a run may take, or why it cannot be given enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    /// It may take this long.
    Run(Duration),
    /// The budget has too little left for it.
    NoTimeLeft {
        /// The step's time so far, since `update` started.
        used: Duration,
        /// What it could have been given.
        left: Duration,
        /// The least it needs.
        floor: Duration,
    },
}

/// The limit of a run of `step` starting at `now`, in a step that started
/// at `started`.
pub(super) fn limit(step: Step, started: SystemTime, now: SystemTime) -> Limit {
    let used = now.duration_since(started).unwrap_or(Duration::ZERO);
    let left = BUDGET.saturating_sub(used).saturating_sub(RESERVE);
    let given = cap(step).map_or(left, |cap| cap.min(left));
    if given < floor(step) {
        Limit::NoTimeLeft {
            used,
            left,
            floor: floor(step),
        }
    } else {
        Limit::Run(given)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: SystemTime = SystemTime::UNIX_EPOCH;

    fn at(seconds: u64) -> SystemTime {
        START + Duration::from_secs(seconds)
    }

    #[test]
    fn the_budget_is_the_tightest_jobs_remainder_less_the_build() {
        assert_eq!(BUDGET, Duration::from_secs(13 * 60 + 42));
        assert_eq!(RESERVE, Duration::from_secs(40));
    }

    #[test]
    fn update_and_the_listing_have_their_caps() {
        assert_eq!(
            limit(Step::Update, START, at(0)),
            Limit::Run(Duration::from_mins(4))
        );
        assert_eq!(
            limit(Step::List, START, at(20)),
            Limit::Run(Duration::from_secs(20))
        );
    }

    /// The install takes what is left: a miss known in seconds, after a
    /// quick update, leaves it twelve and a half minutes; after every
    /// earlier run at its cap with its grace, eight.
    #[test]
    fn the_install_is_given_what_is_left_less_the_reserve() {
        assert_eq!(
            limit(Step::Install, START, at(25)),
            Limit::Run(Duration::from_secs(13 * 60 + 42 - 25 - 40))
        );
        let earlier = 10 + 10 + 240 + 10 + 20 + 10;
        assert_eq!(
            limit(Step::Install, START, at(earlier)),
            Limit::Run(Duration::from_secs(13 * 60 + 42 - earlier - 40))
        );
    }

    /// The invariant: no run is given more than what remains less the
    /// reserve, however late it starts.
    #[test]
    fn no_run_outlasts_the_budget_less_the_reserve() {
        for step in [Step::Update, Step::List, Step::Install] {
            for seconds in (0..=BUDGET.as_secs() + 60).step_by(7) {
                if let Limit::Run(given) = limit(step, START, at(seconds)) {
                    let remaining = BUDGET.saturating_sub(Duration::from_secs(seconds));
                    assert!(
                        given + RESERVE <= remaining,
                        "{step} at {seconds}s: {given:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn too_little_left_is_a_verdict_not_a_short_limit() {
        let late = BUDGET.as_secs() - 40 - 119;
        assert_eq!(
            limit(Step::Install, START, at(late)),
            Limit::NoTimeLeft {
                used: Duration::from_secs(late),
                left: Duration::from_secs(119),
                floor: INSTALL_FLOOR,
            }
        );
        assert_eq!(
            limit(Step::Install, START, at(late - 1)),
            Limit::Run(INSTALL_FLOOR)
        );
        // A capped run is given its cap or nothing.
        assert!(matches!(
            limit(Step::Update, START, at(BUDGET.as_secs() - 40 - 119)),
            Limit::NoTimeLeft { .. }
        ));
    }

    /// A clock that went back is read as no time used.
    #[test]
    fn a_clock_before_the_start_uses_no_time() {
        assert_eq!(
            limit(Step::Update, at(100), at(0)),
            Limit::Run(Duration::from_mins(4))
        );
    }
}
