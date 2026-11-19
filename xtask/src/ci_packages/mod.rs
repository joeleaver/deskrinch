//! `cargo xtask ci-packages`: installs the Ubuntu packages a CI job needs,
//! failing fast, and saying so, when the package servers cannot be reached.
//!
//! It runs `apt-get update`, then `apt-get install --yes
//! --no-install-recommends <packages>`, as root, each with apt's own retry
//! and request timeouts ([`APT_OPTIONS`]) and each cut off after its limit
//! ([`limit`]) by `timeout` running as root ([`bounded`]; apt has no
//! option for a name that does not resolve). apt's own timeouts end a
//! request that gets no answer; the limits bound what they do not: a name
//! that never resolves, which shows at `update`, the first contact, and a
//! mirror that answers but serves slowly, which shows at `install`, where
//! the files are. A run cut off while still fetching is reported as a slow
//! mirror, not an unreachable one. What each run's
//! end means is [`classify::classify`]'s, and is reported as GitHub
//! annotations. It installs packages with `sudo`, so it runs only on CI
//! (`GITHUB_ACTIONS=true`) and refuses everywhere else.

mod annotation;
mod bounded;
mod classify;
mod progress;

use std::time::Duration;

use self::annotation::annotation;
use self::classify::{Ended, Step, Verdict, classify};
use crate::Outcome;
use crate::error::Error;

/// apt's own limits: each request gives up after 20 s without an answer and
/// is tried twice more.
const APT_OPTIONS: [&str; 6] = [
    "-o",
    "Acquire::Retries=2",
    "-o",
    "Acquire::http::Timeout=20",
    "-o",
    "Acquire::https::Timeout=20",
];

/// How long `apt-get` may run for `step`, so that the step (xtask's build,
/// both runs and both runs' [`bounded::KILL_AFTER`]) ends before GitHub ends
/// the tightest job; the test `the_whole_step_fits_the_tightest_job` holds
/// the figures.
///
/// - `update` fetches about 3.5 MB of indexes, in 2 to 15 s on every night
///   seen, and is where a server that cannot be reached shows: two minutes.
/// - `install` fetches 4 to 115 MB. On 2026-11-18 the mirror served
///   steadily slowly, with no fetch stalled and nothing retried: 97 kB/s
///   for labwc's 53.3 MB (109 of 122 files begun when cut off at five
///   minutes, about 9 minutes needed) and 72 kB/s for sway's 52.2 MB (43 of
///   117, about 12 minutes needed). Eleven minutes, what the tightest jobs
///   leave. One limit for every job rather than one each: the jobs that
///   could afford more fetch the small sets, and KWin's job, which needs
///   more (115 MB, 20 to 27 minutes at those rates), has 16.2 minutes: on
///   such a night it fails as slow, and caching the packages is the cure.
fn limit(step: Step) -> Duration {
    match step {
        Step::Update => Duration::from_mins(2),
        Step::Install => Duration::from_mins(11),
    }
}

/// The variable GitHub Actions sets to `true` in every job.
const CI_VARIABLE: &str = "GITHUB_ACTIONS";

/// Installs `packages`, on CI only.
pub(crate) fn run(packages: &[String]) -> Result<Outcome, Error> {
    on_ci(std::env::var(CI_VARIABLE).ok().as_deref())?;
    let mut update_sign = None;
    for step in [Step::Update, Step::Install] {
        let (ended, output) = apt(step, packages)?;
        let verdict = classify(step, ended, &output, update_sign.as_deref());
        if let Some(line) = annotation(&verdict) {
            println!("{line}");
        }
        match verdict {
            Verdict::Done => {}
            Verdict::IndexNotUpdated(sign) => update_sign = Some(sign),
            Verdict::Unreachable(_)
            | Verdict::Slow(_)
            | Verdict::CutOffAfterDownload { .. }
            | Verdict::Failed { .. } => return Ok(Outcome::Failed),
        }
    }
    Ok(Outcome::Passed)
}

/// Whether [`CI_VARIABLE`]'s value, `value`, says this is a CI job.
fn on_ci(value: Option<&str>) -> Result<(), Error> {
    if value == Some("true") {
        Ok(())
    } else {
        Err(Error::NotInCi)
    }
}

/// The arguments of `apt-get` for `step`.
fn arguments(step: Step, packages: &[String]) -> Vec<String> {
    let mut args = Vec::new();
    args.extend(APT_OPTIONS.iter().map(|&option| option.to_owned()));
    match step {
        Step::Update => args.push("update".into()),
        Step::Install => {
            args.extend(["install", "--yes", "--no-install-recommends"].map(String::from));
            args.extend(packages.iter().cloned());
        }
    }
    args
}

/// Runs `apt-get` for `step` as root, within its [`limit`].
fn apt(step: Step, packages: &[String]) -> Result<(Ended, String), Error> {
    bounded::run_as_root("apt-get", &arguments(step, packages), limit(step))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_outside_ci() {
        for value in [None, Some(""), Some("false"), Some("1"), Some("TRUE")] {
            assert!(matches!(on_ci(value), Err(Error::NotInCi)), "{value:?}");
        }
        assert!(on_ci(Some("true")).is_ok());
    }

    #[test]
    fn installs_with_apts_limits_and_no_recommends() {
        let packages = ["sway".to_owned(), "dbus-daemon".to_owned()];
        assert_eq!(
            arguments(Step::Install, &packages).join(" "),
            "-o Acquire::Retries=2 -o Acquire::http::Timeout=20 \
             -o Acquire::https::Timeout=20 install --yes --no-install-recommends sway dbus-daemon"
        );
        assert_eq!(arguments(Step::Update, &packages).last().unwrap(), "update");
    }

    #[test]
    fn update_is_bounded_short_and_install_for_a_slow_mirror() {
        assert_eq!(limit(Step::Update), Duration::from_mins(2));
        assert_eq!(limit(Step::Install), Duration::from_mins(11));
    }

    /// What the tightest jobs leave the package step before GitHub ends them:
    /// the Tests job and sway's compositor job, a 30 and a 45 minute limit less
    /// their other steps at their longest over CI's runs of 2026-11-15 to 18
    /// (cold caches included). A verdict is printed only if the step ends
    /// within it.
    const TIGHTEST_JOB_REMAINDER: Duration = Duration::from_secs(14 * 60 + 42);

    /// The step's time before apt's first line: building xtask, which runs it.
    /// At most 44 s over the 220 job logs of 2026-11-17 and 18 that reached
    /// apt (median 9 s).
    const BUILD_ALLOWANCE: Duration = Duration::from_mins(1);

    /// The step ends, with its verdict printed, before GitHub ends the
    /// tightest job: the build, both runs, and both runs' grace after
    /// `SIGTERM`.
    #[test]
    fn the_whole_step_fits_the_tightest_job() {
        let step = BUILD_ALLOWANCE
            + limit(Step::Update)
            + bounded::KILL_AFTER
            + limit(Step::Install)
            + bounded::KILL_AFTER;
        assert!(
            step <= TIGHTEST_JOB_REMAINDER,
            "{step:?} against {TIGHTEST_JOB_REMAINDER:?}"
        );
    }
}
