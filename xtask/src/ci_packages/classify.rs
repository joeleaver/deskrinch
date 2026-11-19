//! What an `apt-get` run's end means: the pure part of `ci-packages`.

use std::fmt;
use std::time::Duration;

/// Which `apt-get` command ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// `apt-get update`.
    Update,
    /// `apt-get install`.
    Install,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Update => "update",
            Self::Install => "install",
        })
    }
}

/// How an `apt-get` run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// It exited with this status.
    Exited(i32),
    /// A signal ended it.
    Signalled(i32),
    /// It did not finish within the limit and was stopped.
    CutOff(Duration),
}

impl fmt::Display for Ended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited(status) => write!(f, "exited with status {status}"),
            Self::Signalled(signal) => write!(f, "was ended by signal {signal}"),
            Self::CutOff(limit) => write!(f, "was stopped after {}s", limit.as_secs()),
        }
    }
}

/// What the run means for the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// It did its work.
    Done,
    /// `apt-get update` finished, but could not refresh an index (the
    /// runner image lists sources a job may not need): go on, and remember
    /// what it could not reach.
    IndexNotUpdated(String),
    /// The package servers could not be reached; re-running the job is the
    /// remedy.
    Unreachable(String),
    /// The run was cut off while apt was still downloading: the servers
    /// answered, and served slowly.
    Slow(Progress),
    /// It failed for another reason.
    Failed {
        /// The command.
        step: Step,
        /// How it ended.
        ended: Ended,
    },
}

/// What a run cut off while still downloading had fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Progress {
    /// The command.
    pub(crate) step: Step,
    /// How long it had run when it was cut off.
    pub(crate) limit: Duration,
    /// The files it had begun to fetch (apt's `Get:` lines).
    pub(crate) files: usize,
    /// Their sizes, as apt states them, in kB.
    pub(crate) kilobytes: u64,
    /// The last of them, as apt names it.
    pub(crate) last: String,
}

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.limit.as_secs().max(1);
        write!(
            f,
            "apt-get {} was cut off after {seconds}s while still downloading: {} files \
             begun, {} kB, about {} kB/s; the last {}",
            self.step,
            self.files,
            self.kilobytes,
            self.kilobytes / seconds,
            self.last
        )
    }
}

/// What `output` shows of the files apt began to fetch: its `Get:` lines,
/// such as `Get:42 http://archive.ubuntu.com/ubuntu resolute/main amd64
/// libgtk-3-dev amd64 3.24.43-1 [1,810 kB]`; `None` when there are none.
fn progress(step: Step, limit: Duration, output: &str) -> Option<Progress> {
    let gets: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("Get:"))
        .collect();
    let last = gets.last()?;
    Some(Progress {
        step,
        limit,
        files: gets.len(),
        kilobytes: gets.iter().filter_map(|line| kilobytes(line)).sum(),
        last: last
            .split_whitespace()
            .nth(4)
            .unwrap_or_else(|| last.trim())
            .to_owned(),
    })
}

/// The size at the end of a `Get:` line (`[1,810 kB]`, `[2.1 MB]`, `[912 B]`),
/// in kB.
fn kilobytes(line: &str) -> Option<u64> {
    let (_, size) = line.trim_end().strip_suffix(']')?.rsplit_once('[')?;
    let (number, unit) = size.split_once(' ')?;
    let number: f64 = number.replace(',', "").parse().ok()?;
    let factor = match unit {
        "B" => 0.001,
        "kB" => 1.0,
        "MB" => 1000.0,
        _ => return None,
    };
    // A file's size in kB, rounded: far inside u64.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a package's size in kB, non-negative and far below u64::MAX"
    )]
    Some((number * factor).round() as u64)
}

/// The signs in apt's output of a server it could not reach, each up to the
/// end of its detail: a quoted host, or the next word.
const SIGNS: [(&str, Detail); 6] = [
    ("Temporary failure resolving ", Detail::Quoted),
    ("Could not resolve ", Detail::Quoted),
    ("Could not connect to ", Detail::Word),
    ("Connection timed out", Detail::None),
    ("Failed to fetch ", Detail::Word),
    ("Unable to fetch some archives", Detail::None),
];

/// What follows a sign that belongs in the report.
#[derive(Debug, Clone, Copy)]
enum Detail {
    Quoted,
    Word,
    None,
}

/// The first sign in `output` of a server apt could not reach.
pub(crate) fn unreachable_sign(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        SIGNS.iter().find_map(|&(sign, detail)| {
            let at = line.find(sign)?;
            let rest = &line[at + sign.len()..];
            let detail = match detail {
                Detail::Quoted => rest
                    .strip_prefix('\'')
                    .and_then(|quoted| quoted.split_once('\''))
                    .map_or(String::new(), |(host, _)| format!("'{host}'")),
                Detail::Word => rest.split_whitespace().next().unwrap_or("").to_owned(),
                Detail::None => String::new(),
            };
            Some(format!("{sign}{detail}").trim_end().to_owned())
        })
    })
}

/// What a run of `step` that ended as `ended`, printing `output`, means;
/// `update_sign` is what an earlier `apt-get update` could not reach.
pub(crate) fn classify(
    step: Step,
    ended: Ended,
    output: &str,
    update_sign: Option<&str>,
) -> Verdict {
    if let Ended::CutOff(limit) = ended {
        // Still fetching when cut off is a slow mirror; a run that fetched
        // nothing in all that time reached nothing.
        return progress(step, limit, output).map_or_else(
            || {
                Verdict::Unreachable(format!(
                    "apt-get {step} did not finish within {}s",
                    limit.as_secs()
                ))
            },
            Verdict::Slow,
        );
    }
    let sign = unreachable_sign(output);
    match (sign, ended, step) {
        (Some(sign), Ended::Exited(0), Step::Update) => Verdict::IndexNotUpdated(sign),
        (Some(sign), _, _) => Verdict::Unreachable(sign),
        (None, Ended::Exited(0), _) => Verdict::Done,
        (None, _, _) => match update_sign {
            Some(sign) => Verdict::Unreachable(sign.to_owned()),
            None => Verdict::Failed { step, ended },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the runners printed on 2026-11-11, when they could not resolve
    /// Ubuntu's hosts.
    const RUNNER: &str = "\
Ign:1 http://azure.archive.ubuntu.com/ubuntu resolute InRelease
Err:1 http://azure.archive.ubuntu.com/ubuntu resolute InRelease
  Temporary failure resolving 'azure.archive.ubuntu.com'
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/main/l/libx/libxrandr-dev_1.5.4-1build1_amd64v3.deb  Temporary failure resolving 'security.ubuntu.com'
E: Unable to fetch some archives, maybe run apt update or try with --fix-missing?
";

    #[test]
    fn the_runners_outage_is_unreachable() {
        assert_eq!(
            classify(Step::Install, Ended::Exited(100), RUNNER, None),
            Verdict::Unreachable("Temporary failure resolving 'azure.archive.ubuntu.com'".into())
        );
        let archives = "E: Unable to fetch some archives, maybe run apt update?\n";
        assert_eq!(
            classify(Step::Install, Ended::Exited(100), archives, None),
            Verdict::Unreachable("Unable to fetch some archives".into())
        );
    }

    #[test]
    fn an_unresolvable_mirror_only_warns_on_update() {
        let output = "Err:1 http://nonexistent.invalid/ubuntu resolute InRelease\n  Could not resolve 'nonexistent.invalid'\nW: Failed to fetch http://nonexistent.invalid/ubuntu/dists/resolute/InRelease  Could not resolve 'nonexistent.invalid'\n";
        assert_eq!(
            classify(Step::Update, Ended::Exited(0), output, None),
            Verdict::IndexNotUpdated("Could not resolve 'nonexistent.invalid'".into())
        );
    }

    #[test]
    fn an_unanswering_mirror_cut_off_is_unreachable() {
        assert_eq!(
            classify(
                Step::Update,
                Ended::CutOff(Duration::from_secs(300)),
                "",
                None
            ),
            Verdict::Unreachable("apt-get update did not finish within 300s".into())
        );
    }

    /// The last files PR 142's sway job had begun when its install was cut
    /// off on 2026-11-18: the mirror served steadily, at about 70 kB/s.
    const SLOW: &str = "\
Get:6 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/main amd64v3 libc6 amd64 2.43-2ubuntu2.7 [2095 kB]
Get:7 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/main amd64v3 libc-bin amd64 2.43-2ubuntu2.7 [716 kB]
Get:8 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/main amd64v3 locales all 2.43-2ubuntu2.7 [4226 kB]
";

    #[test]
    fn a_mirror_still_serving_when_cut_off_is_slow_not_unreachable() {
        let limit = Duration::from_secs(600);
        let verdict = classify(Step::Install, Ended::CutOff(limit), SLOW, None);
        assert_eq!(
            verdict,
            Verdict::Slow(Progress {
                step: Step::Install,
                limit,
                files: 3,
                kilobytes: 2095 + 716 + 4226,
                last: "locales".into(),
            })
        );
        let Verdict::Slow(progress) = verdict else {
            unreachable!()
        };
        let said = progress.to_string();
        assert!(said.contains("while still downloading"), "{said}");
        assert!(said.contains("about 11 kB/s"), "{said}");
        assert!(!said.contains("re-run"), "{said}");
    }

    #[test]
    fn a_run_that_fetched_nothing_before_its_cut_off_is_unreachable() {
        let stalled = "Ign:1 http://azure.archive.ubuntu.com/ubuntu resolute InRelease\n";
        assert_eq!(
            classify(
                Step::Update,
                Ended::CutOff(Duration::from_secs(300)),
                stalled,
                None
            ),
            Verdict::Unreachable("apt-get update did not finish within 300s".into())
        );
    }

    #[test]
    fn sizes_read_in_each_unit() {
        assert_eq!(kilobytes("Get:1 x [912 B]"), Some(1));
        assert_eq!(kilobytes("Get:1 x [1,810 kB]"), Some(1810));
        assert_eq!(kilobytes("Get:1 x [2.1 MB]"), Some(2100));
        assert_eq!(kilobytes("Get:1 x"), None);
    }

    #[test]
    fn an_unanswering_mirror_within_the_limit_warns() {
        let output = "W: Failed to fetch http://10.255.255.1/ubuntu/dists/resolute/InRelease  Could not connect to 10.255.255.1:80 (10.255.255.1), connection timed out\n";
        assert_eq!(
            classify(Step::Update, Ended::Exited(0), output, None),
            Verdict::IndexNotUpdated("Could not connect to 10.255.255.1:80".into())
        );
    }

    #[test]
    fn an_installed_package_is_done() {
        let output = "0 upgraded, 0 newly installed, 0 to remove and 0 not upgraded.\n";
        assert_eq!(
            classify(Step::Install, Ended::Exited(0), output, None),
            Verdict::Done
        );
    }

    #[test]
    fn a_missing_package_is_a_failure_not_the_network() {
        let output = "E: Unable to locate package no-such-package-xyz\n";
        assert_eq!(
            classify(Step::Install, Ended::Exited(100), output, None),
            Verdict::Failed {
                step: Step::Install,
                ended: Ended::Exited(100)
            }
        );
    }

    #[test]
    fn an_install_after_an_unrefreshed_index_blames_the_network() {
        let output = "E: Unable to locate package dbus-daemon\n";
        assert_eq!(
            classify(
                Step::Install,
                Ended::Exited(100),
                output,
                Some("Could not resolve 'archive.ubuntu.com'")
            ),
            Verdict::Unreachable("Could not resolve 'archive.ubuntu.com'".into())
        );
    }

    #[test]
    fn how_a_run_ended_reads() {
        assert_eq!(Ended::Exited(100).to_string(), "exited with status 100");
        assert_eq!(Ended::Signalled(9).to_string(), "was ended by signal 9");
        assert_eq!(
            Ended::CutOff(Duration::from_secs(300)).to_string(),
            "was stopped after 300s"
        );
    }

    #[test]
    fn a_signal_is_a_failure() {
        assert_eq!(
            classify(Step::Install, Ended::Signalled(9), "", None),
            Verdict::Failed {
                step: Step::Install,
                ended: Ended::Signalled(9)
            }
        );
    }
}
