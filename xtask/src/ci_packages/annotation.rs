//! The words a verdict puts before the reader: one GitHub annotation per
//! verdict that has something to say, written here and nowhere else.

use super::budget::BUDGET;
use super::classify::{Step, Verdict};
use super::progress::Progress;

/// The annotation `run` prints for `verdict`; `None` for a run that did its
/// work.
pub(crate) fn annotation(verdict: &Verdict) -> Option<String> {
    match verdict {
        Verdict::Done => None,
        Verdict::IndexNotUpdated(sign) => {
            Some(format!("::warning title=Package index not updated::{sign}"))
        }
        Verdict::UpdateCutOff(cut) => Some(format!(
            "::warning title=Package index partly updated::apt-get update was cut off after \
             {}s {}. apt moves a repository's indexes into place only once all of them have \
             come, so each repository still being fetched at the cut is left as the runner \
             image had it, a consistent older set, and the others are fresh. The job goes on \
             with that index; if a planned package depended on a repository left behind, the \
             install's own verdict says so.",
            cut.limit.as_secs(),
            cut.doing()
        )),
        Verdict::Unreachable(reason) => Some(format!(
            "::error title=Package servers unreachable::apt could not reach Ubuntu's package \
             servers ({reason}). This is the runner's network, not the code: re-run the job."
        )),
        Verdict::Slow(progress) => Some(format!(
            "::error title=Package download too slow::{}. The package servers answered and \
             served slowly: not an outage, so a re-run later may pass. Only what the package \
             cache lacked was fetched; once main has cached these packages, they are not \
             fetched at all.",
            slow(progress)
        )),
        Verdict::CutOffAfterDownload {
            step,
            limit,
            downloaded,
        } => Some(format!(
            "::error title=apt-get {step} did not finish::apt-get {step} was cut off after \
             {seconds}s {after}; its download had finished (Fetched {report}). This is not \
             the network: see its output above.",
            seconds = limit.as_secs(),
            after = after_the_download(*step),
            report = downloaded.report,
        )),
        Verdict::Unsandboxed(notice) => Some(format!(
            "::error title=apt downloaded outside its sandbox::{notice} The package cache \
             directory must be one apt's _apt user can reach: see cache.rs in xtask's \
             ci_packages."
        )),
        Verdict::NoTimeLeft {
            step,
            used,
            left,
            floor,
        } => Some(format!(
            "::error title=No time left for apt-get {step}::The package step had used {}s of \
             its {}s when apt-get {step} was to start, leaving it {}s, under the {}s it needs. \
             This is not the mirror: the times of the runs above, and the restore between \
             them, say where the time went.",
            used.as_secs(),
            BUDGET.as_secs(),
            left.as_secs(),
            floor.as_secs()
        )),
        Verdict::Unverifiable { files, first } => Some(format!(
            "::error title=Package checksums not verifiable::{files} of the files apt would \
             fetch have no SHA-256 or SHA-512 checksum in its listing (the first: {first}), so \
             the package cache could keep none of them. The listing asks for SHA-256 \
             (Acquire::ForceHash); see its output above."
        )),
        Verdict::Failed { step, ended } => Some(format!(
            "::error title=apt-get {step} failed::apt-get {step} {ended}; see its output above."
        )),
    }
}

/// How far a slow run got: still downloading, or done downloading and cut
/// off installing.
fn slow(progress: &Progress) -> String {
    let seconds = progress.limit.as_secs().max(1);
    match &progress.downloaded {
        None => format!(
            "apt-get {} was cut off after {seconds}s while still downloading: {} files begun, \
             {} kB, about {} kB/s; the last {}",
            progress.step,
            progress.files,
            progress.kilobytes,
            progress.kilobytes / seconds,
            progress.last
        ),
        Some(downloaded) => format!(
            "apt-get {} was cut off after {seconds}s: its download took most of that time \
             (Fetched {}), and what was left ran out {}",
            progress.step,
            downloaded.report,
            after_the_download(progress.step)
        ),
    }
}

/// What `step` does once its download is done, as a run cut off then was
/// doing.
fn after_the_download(step: Step) -> &'static str {
    match step {
        Step::Update => "while reading the package lists",
        Step::List => "while working out what to fetch",
        Step::Install => "while unpacking and configuring",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::ci_packages::progress::Downloaded;

    fn progress(downloaded: Option<Downloaded>) -> Progress {
        Progress {
            step: Step::Install,
            limit: Duration::from_mins(11),
            files: 43,
            kilobytes: 21392,
            last: "locales".into(),
            downloaded,
        }
    }

    /// What a slow run tells the reader: that it was slow, how far it got,
    /// and never that the servers were unreachable or that a re-run is the
    /// remedy.
    #[test]
    fn a_slow_run_says_slow_and_not_unreachable() {
        let said = annotation(&Verdict::Slow(progress(None))).unwrap();
        assert!(
            said.starts_with("::error title=Package download too slow::"),
            "{said}"
        );
        assert!(
            said.contains(
                "while still downloading: 43 files begun, 21392 kB, about 32 kB/s; the last locales"
            ),
            "{said}"
        );
        assert!(!said.to_lowercase().contains("unreachable"), "{said}");
        assert!(!said.to_lowercase().contains("could not reach"), "{said}");
        assert!(!said.contains("re-run the job"), "{said}");
    }

    #[test]
    fn a_slow_download_that_finished_says_so() {
        let downloaded = Downloaded {
            report: "15.4 MB in 6min 2s (55.6 kB/s)".into(),
            took: Duration::from_secs(362),
        };
        let said = annotation(&Verdict::Slow(progress(Some(downloaded)))).unwrap();
        assert!(
            said.contains(
                "its download took most of that time (Fetched 15.4 MB in 6min 2s (55.6 kB/s))"
            ),
            "{said}"
        );
        assert!(said.contains("unpacking and configuring"), "{said}");
        assert!(!said.contains("still downloading"), "{said}");
    }

    #[test]
    fn a_cut_off_installing_does_not_blame_the_network() {
        let downloaded = Downloaded {
            report: "15.4 MB in 8s (1925 kB/s)".into(),
            took: Duration::from_secs(8),
        };
        let said = annotation(&Verdict::CutOffAfterDownload {
            step: Step::Install,
            limit: Duration::from_mins(11),
            downloaded,
        })
        .unwrap();
        assert!(
            said.starts_with("::error title=apt-get install did not finish::"),
            "{said}"
        );
        assert!(said.contains("while unpacking and configuring"), "{said}");
        assert!(
            said.contains("(Fetched 15.4 MB in 8s (1925 kB/s))"),
            "{said}"
        );
        assert!(said.contains("This is not the network"), "{said}");
        assert!(!said.contains("slow"), "{said}");
    }

    /// An install with too little of the budget left says where its time
    /// went, and does not blame the mirror.
    #[test]
    fn no_time_left_says_so_and_not_slow() {
        let said = annotation(&Verdict::NoTimeLeft {
            step: Step::Install,
            used: Duration::from_secs(703),
            left: Duration::from_secs(79),
            floor: Duration::from_mins(2),
        })
        .unwrap();
        assert!(
            said.starts_with("::error title=No time left for apt-get install::"),
            "{said}"
        );
        assert!(
            said.contains("used 703s of its 822s when apt-get install was to start, leaving it 79s, under the 120s it needs"),
            "{said}"
        );
        assert!(said.contains("This is not the mirror"), "{said}");
        assert!(!said.contains("slow"), "{said}");
    }

    /// A cut-off update that goes on warns in its own words: a stalled
    /// host named, not a slow mirror, and the repositories left behind.
    #[test]
    fn a_cut_off_update_warns_of_the_stalled_host_and_goes_on() {
        let said = annotation(&Verdict::UpdateCutOff(crate::ci_packages::cut::UpdateCut {
            limit: Duration::from_secs(240),
            stalled: Some(crate::ci_packages::cut::Stall {
                host: "security.ubuntu.com".into(),
                failures: 3,
                last: "resolute-security/main amd64v3 Packages".into(),
            }),
            last_host: "security.ubuntu.com".into(),
        }))
        .unwrap();
        assert!(
            said.starts_with(
                "::warning title=Package index partly updated::apt-get update was cut off after \
                 240s with security.ubuntu.com stalled (3 failed attempts"
            ),
            "{said}"
        );
        assert!(said.contains("left as the runner image had it"), "{said}");
        assert!(said.contains("The job goes on"), "{said}");
        assert!(!said.contains("slow"), "{said}");
    }

    #[test]
    fn unverifiable_checksums_say_the_cache_could_keep_nothing() {
        let said = annotation(&Verdict::Unverifiable {
            files: 97,
            first: "gir1.2-atk-1.0_2.60.4-0ubuntu0.1_amd64v3.deb MD5Sum:975c86779ce8869032f1d82004544fba"
                .into(),
        })
        .unwrap();
        assert!(
            said.starts_with("::error title=Package checksums not verifiable::97 of the files"),
            "{said}"
        );
        assert!(said.contains("(the first: gir1.2-atk-1.0_"), "{said}");
        assert!(said.contains("could keep none of them"), "{said}");
    }

    /// `update` has a `Fetched` line too: after it, apt reads the package
    /// lists, and says so.
    #[test]
    fn an_update_cut_off_after_its_download_is_reading_the_lists() {
        let downloaded = Downloaded {
            report: "3486 kB in 7s (498 kB/s)".into(),
            took: Duration::from_secs(7),
        };
        let said = annotation(&Verdict::CutOffAfterDownload {
            step: Step::Update,
            limit: Duration::from_mins(2),
            downloaded,
        })
        .unwrap();
        assert!(
            said.starts_with("::error title=apt-get update did not finish::"),
            "{said}"
        );
        assert!(said.contains("while reading the package lists"), "{said}");
        assert!(!said.contains("unpacking"), "{said}");
        let slow = Progress {
            step: Step::Update,
            ..progress(Some(Downloaded {
                report: "3486 kB in 1min 30s (38.7 kB/s)".into(),
                took: Duration::from_secs(90),
            }))
        };
        let said = annotation(&Verdict::Slow(slow)).unwrap();
        assert!(
            said.contains("ran out while reading the package lists"),
            "{said}"
        );
    }

    #[test]
    fn an_unreachable_server_is_said_as_before() {
        assert_eq!(
            annotation(&Verdict::Unreachable(
                "Could not resolve 'archive.ubuntu.com'".into()
            )),
            Some(
                "::error title=Package servers unreachable::apt could not reach Ubuntu's \
                 package servers (Could not resolve 'archive.ubuntu.com'). This is the \
                 runner's network, not the code: re-run the job."
                    .into()
            )
        );
        assert_eq!(annotation(&Verdict::Done), None);
    }
}
