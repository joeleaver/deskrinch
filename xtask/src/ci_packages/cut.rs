//! An `apt-get update` cut off after it had fetched something: where it
//! was, as its output shows, and why the job can go on.
//!
//! apt updates its index one repository at a time, as a transaction: a
//! repository's new files are staged in `lists/partial/` and moved into
//! place only once all of them have come, its `InRelease` last
//! (`pkgAcqMetaBase::CommitTransaction`). So a cut-off leaves each
//! repository still being fetched as the runner image had it, a
//! consistent, signed, older set, and every one already committed fresh:
//! what apt itself leaves when a host fails within its retries, which goes
//! on too (`IndexNotUpdated`). An `update` cut off before it had fetched
//! anything reached nothing, and stays unreachable.

use std::collections::BTreeMap;
use std::time::Duration;

use super::progress::fetched_item;

/// Where a cut-off `update` was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateCut {
    /// How long it had run.
    pub(crate) limit: Duration,
    /// The host it was retrying, if its output shows retries.
    pub(crate) stalled: Option<Stall>,
    /// The host of its last fetch.
    pub(crate) last_host: String,
}

/// A host whose fetches apt was retrying: `Ign:` lines against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stall {
    /// The host.
    pub(crate) host: String,
    /// How many attempts at its files failed.
    pub(crate) failures: usize,
    /// The last of them.
    pub(crate) last: String,
}

impl UpdateCut {
    /// What `output`, an `update` cut off after `limit`, shows of where it
    /// was.
    pub(crate) fn read(limit: Duration, output: &str) -> Self {
        let fetches: Vec<(&str, String)> = output
            .lines()
            .filter_map(|line| Some((line, host(line)?)))
            .collect();
        let mut failures: BTreeMap<&str, (usize, &str)> = BTreeMap::new();
        for (line, host) in &fetches {
            if line.starts_with("Ign:") {
                let entry = failures.entry(host.as_str()).or_insert((0, line));
                entry.0 += 1;
                entry.1 = line;
            }
        }
        let stalled = failures
            .into_iter()
            .max_by_key(|(_, (count, _))| *count)
            .map(|(host, (failures, line))| Stall {
                host: host.to_owned(),
                failures,
                last: fetched_item(line),
            });
        Self {
            limit,
            stalled,
            last_host: fetches
                .last()
                .map_or_else(String::new, |(_, host)| host.clone()),
        }
    }

    /// What it was doing at the cut, in a few words.
    pub(crate) fn doing(&self) -> String {
        match &self.stalled {
            Some(stall) => format!(
                "with {} stalled ({} failed attempts at its indexes, the last at {})",
                stall.host, stall.failures, stall.last
            ),
            None => format!("while still fetching from {}", self.last_host),
        }
    }

    /// The sign the install's verdict is given if a planned file depended
    /// on a repository left behind.
    pub(crate) fn sign(&self) -> String {
        format!(
            "apt-get update was cut off after {}s {}",
            self.limit.as_secs(),
            self.doing()
        )
    }
}

/// The host a fetch line (`Get:`, `Hit:`, `Ign:`, `Err:`) of `update` names,
/// from its URL; `None` for any other line, or a URL with no host (apt's
/// mirror list, a `file:` one).
fn host(line: &str) -> Option<String> {
    let (tag, rest) = line.split_once(':')?;
    if !matches!(tag, "Get" | "Hit" | "Ign" | "Err") {
        return None;
    }
    let url = rest.split_whitespace().nth(1)?;
    let (_, after) = url.split_once("://")?;
    let host = after.split('/').next()?;
    (!host.is_empty()).then(|| host.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_packages::classify::{Ended, Step, Verdict, classify};

    /// What apt-get update printed in the Documentation job of PR 156 (run
    /// 44642846821, first attempt, 2026-11-20 00:05 UTC) before it was cut
    /// off at 120 s: the region's mirror served its indexes within 2 s;
    /// `security.ubuntu.com` then stalled.
    const STALLED: &str = "\
Get:1 file:/etc/apt/apt-mirrors.txt Mirrorlist [152 B]
Get:5 https://packages.microsoft.com/ubuntu/26.04/prod resolute InRelease [3964 B]
Hit:2 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute InRelease
Get:6 https://packages.microsoft.com/ubuntu/26.04/prod resolute/main amd64 Packages [165 kB]
Get:3 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates InRelease [137 kB]
Get:7 https://packages.microsoft.com/ubuntu/26.04/prod resolute/main arm64 Packages [151 kB]
Get:4 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-backports InRelease [137 kB]
Get:8 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/main amd64v3 Packages [901 kB]
Get:9 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/main Translation-en [200 kB]
Get:10 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/universe amd64v3 Packages [394 kB]
Get:11 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/universe Translation-en [124 kB]
Get:12 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/restricted amd64v3 Packages [629 kB]
Get:13 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/restricted Translation-en [130 kB]
Get:14 http://security.ubuntu.com/ubuntu resolute-security InRelease [137 kB]
Ign:15 http://security.ubuntu.com/ubuntu resolute-security/main amd64v3 Packages
Get:16 http://security.ubuntu.com/ubuntu resolute-security/main Translation-en [158 kB]
Get:17 http://security.ubuntu.com/ubuntu resolute-security/universe amd64v3 Packages [222 kB]
Ign:18 http://security.ubuntu.com/ubuntu resolute-security/universe Translation-en
Ign:15 http://security.ubuntu.com/ubuntu resolute-security/main amd64v3 Packages
Get:18 http://security.ubuntu.com/ubuntu resolute-security/universe Translation-en [72.7 kB]
";

    const LIMIT: Duration = Duration::from_secs(120);

    #[test]
    fn a_cut_off_update_with_progress_goes_on_naming_the_stalled_host() {
        let verdict = classify(Step::Update, Ended::CutOff(LIMIT), STALLED, None);
        let Verdict::UpdateCutOff(cut) = verdict else {
            panic!("not gone on: {verdict:?}");
        };
        assert_eq!(
            cut.stalled,
            Some(Stall {
                host: "security.ubuntu.com".into(),
                failures: 3,
                last: "resolute-security/main amd64v3 Packages".into(),
            })
        );
        assert_eq!(
            cut.sign(),
            "apt-get update was cut off after 120s with security.ubuntu.com stalled (3 failed \
             attempts at its indexes, the last at resolute-security/main amd64v3 Packages)"
        );
    }

    #[test]
    fn without_retries_it_names_the_host_still_fetching() {
        let mirror_only = STALLED.lines().take(9).collect::<Vec<_>>().join("\n");
        let cut = UpdateCut::read(LIMIT, &mirror_only);
        assert_eq!(cut.stalled, None);
        assert_eq!(
            cut.doing(),
            "while still fetching from us-east-2.ec2.archive.ubuntu.com"
        );
    }

    #[test]
    fn a_cut_off_update_that_fetched_nothing_is_unreachable() {
        let nothing = "Ign:1 http://azure.archive.ubuntu.com/ubuntu resolute InRelease\n";
        assert_eq!(
            classify(Step::Update, Ended::CutOff(LIMIT), nothing, None),
            Verdict::Unreachable("apt-get update did not finish within 120s".into())
        );
        assert_eq!(
            classify(Step::Update, Ended::CutOff(LIMIT), "", None),
            Verdict::Unreachable("apt-get update did not finish within 120s".into())
        );
    }

    /// The install after it, failing on a file the repository left behind
    /// named (gone from the pool), says so with the update's warning
    /// beside it.
    #[test]
    fn an_install_failing_after_it_carries_the_update_s_warning() {
        let cut = UpdateCut::read(LIMIT, STALLED);
        let gone = "E: Failed to fetch http://security.ubuntu.com/ubuntu/pool/main/g/glib2.0/libglib2.0-0t64_2.88.0-1ubuntu0.1_amd64v3.deb  404  Not Found [IP: 91.189.91.83 80]\n";
        assert_eq!(
            classify(Step::Install, Ended::Exited(100), gone, Some(&cut.sign())),
            Verdict::Unreachable(format!(
                "Failed to fetch http://security.ubuntu.com/ubuntu/pool/main/g/glib2.0/libglib2.0-0t64_2.88.0-1ubuntu0.1_amd64v3.deb; {}",
                cut.sign()
            ))
        );
        // And with no sign of its own, the update's.
        assert_eq!(
            classify(
                Step::Install,
                Ended::Exited(100),
                "E: Unable to locate package dbus-daemon\n",
                Some(&cut.sign())
            ),
            Verdict::Unreachable(cut.sign())
        );
    }

    #[test]
    fn a_fetch_line_s_host() {
        assert_eq!(
            host(
                "Ign:15 http://security.ubuntu.com/ubuntu resolute-security/main amd64v3 Packages"
            ),
            Some("security.ubuntu.com".into())
        );
        assert_eq!(
            host("Get:1 file:/etc/apt/apt-mirrors.txt Mirrorlist [152 B]"),
            None
        );
        assert_eq!(host("Reading package lists..."), None);
        assert_eq!(host("W: Failed to fetch http://host/x"), None);
    }
}
