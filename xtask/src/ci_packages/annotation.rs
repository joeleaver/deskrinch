//! The words a verdict puts before the reader: one GitHub annotation per
//! verdict that has something to say, written here and nowhere else.

use super::classify::Verdict;
use super::progress::Progress;

/// The annotation `run` prints for `verdict`; `None` for a run that did its
/// work.
pub(crate) fn annotation(verdict: &Verdict) -> Option<String> {
    match verdict {
        Verdict::Done => None,
        Verdict::IndexNotUpdated(sign) => {
            Some(format!("::warning title=Package index not updated::{sign}"))
        }
        Verdict::Unreachable(reason) => Some(format!(
            "::error title=Package servers unreachable::apt could not reach Ubuntu's package \
             servers ({reason}). This is the runner's network, not the code: re-run the job."
        )),
        Verdict::Slow(progress) => Some(format!(
            "::error title=Package download too slow::{}. The package servers answered and \
             served slowly: not an outage, so a re-run later may pass, and caching the packages \
             is the cure.",
            slow(progress)
        )),
        Verdict::CutOffInstalling(progress) => Some(format!(
            "::error title=apt-get {step} did not finish::apt-get {step} was cut off after \
             {seconds}s while unpacking and configuring; its download had finished (Fetched \
             {report}). This is not the network: see its output above.",
            step = progress.step,
            seconds = progress.limit.as_secs(),
            report = progress
                .downloaded
                .as_ref()
                .map_or("", |downloaded| downloaded.report.as_str()),
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
             (Fetched {}), and what was left ran out while unpacking and configuring",
            progress.step, downloaded.report
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::ci_packages::classify::Step;
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
        let said = annotation(&Verdict::CutOffInstalling(progress(Some(downloaded)))).unwrap();
        assert!(
            said.starts_with("::error title=apt-get install did not finish::"),
            "{said}"
        );
        assert!(said.contains("This is not the network"), "{said}");
        assert!(!said.contains("slow"), "{said}");
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
