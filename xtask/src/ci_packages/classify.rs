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
    /// It failed for another reason.
    Failed {
        /// The command.
        step: Step,
        /// How it ended.
        ended: Ended,
    },
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
        return Verdict::Unreachable(format!(
            "apt-get {step} did not finish within {}s",
            limit.as_secs()
        ));
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
