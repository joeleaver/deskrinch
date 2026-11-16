//! The decision, with the clock injected: every time is a base `Instant`
//! plus an offset, and nothing sleeps.

use std::os::unix::process::ExitStatusExt;

use super::*;

const POLICY: Restart = Restart::OnFailure {
    burst: 3,
    interval: Duration::from_secs(60),
};

/// `seconds` after `base`.
fn at(base: Instant, seconds: f64) -> Instant {
    base + Duration::from_secs_f64(seconds)
}

fn exited(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8)
}

fn killed(signal: i32) -> ExitStatus {
    ExitStatus::from_raw(signal)
}

#[test]
fn a_failure_inside_the_limit_restarts() {
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 5.0)),
        Verdict::Restart { start: 2 }
    );
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 10.0)),
        Verdict::Restart { start: 3 }
    );
}

#[test]
fn a_clean_exit_does_not_restart() {
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    assert_eq!(
        starts.after_exit(Exit::Clean, at(base, 5.0)),
        Verdict::Clean
    );
}

#[test]
fn the_third_failure_within_the_window_is_not_restarted_and_stays_so() {
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    starts.after_exit(Exit::Failed, at(base, 1.0));
    starts.after_exit(Exit::Failed, at(base, 2.0));
    let exhausted = Verdict::Exhausted {
        burst: 3,
        interval: Duration::from_secs(60),
    };
    assert_eq!(starts.after_exit(Exit::Failed, at(base, 3.0)), exhausted);
    // Not even long after: systemd leaves the unit failed.
    assert_eq!(starts.after_exit(Exit::Failed, at(base, 600.0)), exhausted);
}

#[test]
fn failures_spread_wider_than_the_window_keep_restarting() {
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    for n in 1..=10 {
        let verdict = starts.after_exit(Exit::Failed, at(base, 61.0 * f64::from(n)));
        assert_eq!(verdict, Verdict::Restart { start: 1 }, "failure {n}");
    }
}

#[test]
fn the_window_is_systemds_from_its_first_start_not_a_sliding_one() {
    // Starts at 0 and 59; the failure at 61 is past the window begun at 0,
    // so it begins a new one (1 start), and the starts at 62 and 63 are its
    // second and third. A sliding window would count 59, 61, 62 and 63
    // within the last 60 s and refuse the start at 63.
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 59.0)),
        Verdict::Restart { start: 2 }
    );
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 61.0)),
        Verdict::Restart { start: 1 }
    );
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 62.0)),
        Verdict::Restart { start: 2 }
    );
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 63.0)),
        Verdict::Restart { start: 3 }
    );
}

#[test]
fn a_start_exactly_one_interval_after_the_window_began_is_still_in_it() {
    // systemd begins a new window only past the interval (`>`).
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 30.0)),
        Verdict::Restart { start: 2 }
    );
    assert_eq!(
        starts.after_exit(Exit::Failed, at(base, 60.0)),
        Verdict::Restart { start: 3 }
    );
}

#[test]
fn never_is_never_restarted() {
    let base = Instant::now();
    let mut starts = Starts::new(Restart::Never, base);
    assert_eq!(starts.after_exit(Exit::Failed, base), Verdict::Never);
    assert_eq!(starts.after_exit(Exit::Clean, base), Verdict::Never);
}

#[test]
fn what_a_failure_is_follows_systemds_on_failure() {
    assert_eq!(Exit::of(&exited(0)), Exit::Clean);
    assert_eq!(Exit::of(&exited(1)), Exit::Failed);
    assert_eq!(Exit::of(&exited(143)), Exit::Failed, "a code, not a signal");
    for clean in [libc::SIGHUP, libc::SIGINT, libc::SIGTERM, libc::SIGPIPE] {
        assert_eq!(Exit::of(&killed(clean)), Exit::Clean, "signal {clean}");
    }
    for failed in [libc::SIGKILL, libc::SIGSEGV, libc::SIGABRT, libc::SIGQUIT] {
        assert_eq!(Exit::of(&killed(failed)), Exit::Failed, "signal {failed}");
    }
    // A clean signal with a core dump is not clean.
    assert_eq!(Exit::of(&killed(libc::SIGTERM | 0x80)), Exit::Failed);
}

#[test]
fn the_policy_maps_to_a_units_lines_as_written() {
    assert_eq!(
        POLICY.systemd_lines(),
        [
            "Restart=on-failure",
            "StartLimitBurst=3",
            "StartLimitIntervalSec=60"
        ]
    );
    assert_eq!(Restart::Never.systemd_lines(), ["Restart=no"]);
}

/// A component whose display went away ends with
/// [`DISPLAY_LOST`](crate::exit_status::DISPLAY_LOST), which the policy
/// reads as a failure: started again, it reaches a compositor that came
/// back on the same socket (KWin's wrapper), and the limit ends the
/// restarts when none does.
#[test]
fn a_lost_display_is_a_failure() {
    let lost = exited(i32::from(crate::exit_status::DISPLAY_LOST));
    assert_eq!(Exit::of(&lost), Exit::Failed);
    let base = Instant::now();
    let mut starts = Starts::new(POLICY, base);
    assert_eq!(
        starts.after_exit(Exit::of(&lost), at(base, 1.0)),
        Verdict::Restart { start: 2 }
    );
}
