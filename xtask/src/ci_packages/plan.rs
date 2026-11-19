//! What a job's packages come to: the files apt would fetch for them, the
//! cache key that names those files, and the plan `ci-packages update`
//! leaves for `ci-packages install`.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::check::hex;
use crate::error::Error;

/// A file apt would fetch, as `apt-get install --print-uris` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Download {
    /// Its name in the archives directory.
    pub(super) file: String,
    /// Its size in bytes.
    pub(super) size: u64,
    /// apt's checksum of it, such as `SHA256:…`.
    pub(super) checksum: String,
}

/// The downloads in `output`, what `apt-get install --print-uris` printed:
/// a line `'URI' file size checksum` for each. apt's other lines (a
/// warning, say) do not start with a quote; a quoted line that does not
/// read as a download is an error, for a key that left it out would name
/// the wrong files.
pub(super) fn downloads(output: &str) -> Result<Vec<Download>, Error> {
    output
        .lines()
        .filter(|line| line.starts_with('\''))
        .map(|line| {
            download(line).ok_or_else(|| Error::AptListing {
                line: line.to_owned(),
            })
        })
        .collect()
}

/// One line of `--print-uris`'s.
fn download(line: &str) -> Option<Download> {
    let (_uri, rest) = line.strip_prefix('\'')?.split_once("' ")?;
    let mut words = rest.split_whitespace();
    let download = Download {
        file: words.next()?.to_owned(),
        size: words.next()?.parse().ok()?,
        checksum: words.next()?.to_owned(),
    };
    words.next().is_none().then_some(download)
}

/// The packages a job installs and the files apt would fetch for them, on
/// this runner image, with nothing cached; and when the package step
/// began, which its budget counts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Plan {
    /// When `ci-packages update` started.
    pub(super) started: SystemTime,
    /// The packages, sorted, each once.
    pub(super) packages: Vec<String>,
    /// The files, sorted by name.
    pub(super) downloads: Vec<Download>,
}

/// The cache key of a plan, and the prefix an older cache of the same
/// packages shares with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Key {
    /// The key the job's packages are saved under.
    pub(super) exact: String,
    /// What a cache of the same packages on the same image and architecture
    /// begins with, whatever files it holds.
    pub(super) prefix: String,
}

impl Plan {
    /// The plan for `packages`, of which apt would fetch `downloads`, in a
    /// step that started at `started`.
    pub(super) fn new(
        started: SystemTime,
        packages: &[String],
        mut downloads: Vec<Download>,
    ) -> Self {
        let packages: BTreeSet<&String> = packages.iter().collect();
        downloads.sort_by(|a, b| a.file.cmp(&b.file));
        Self {
            started,
            packages: packages.into_iter().cloned().collect(),
            downloads,
        }
    }

    /// The key on runner image `image` (GitHub's `ImageOS`) and
    /// architecture `arch`: those, a hash of the packages, and a hash of
    /// the files apt would fetch (name, size and checksum). The image's
    /// version is not in it: the files are what an image update changes.
    pub(super) fn key(&self, image: &str, arch: &str) -> Key {
        let packages = hash(self.packages.iter().map(String::as_str));
        let files = hash(
            self.downloads
                .iter()
                .map(|d| format!("{} {} {}", d.file, d.size, d.checksum)),
        );
        let prefix = format!("apt-{image}-{arch}-{packages}-");
        Key {
            exact: format!("{prefix}{files}"),
            prefix,
        }
    }

    /// The names of the files apt would fetch.
    pub(super) fn files(&self) -> BTreeSet<&str> {
        self.downloads.iter().map(|d| d.file.as_str()).collect()
    }

    /// What the files come to, in bytes.
    pub(super) fn bytes(&self) -> u64 {
        self.downloads.iter().map(|d| d.size).sum()
    }

    /// Writes the plan to `path`.
    pub(super) fn save(&self, path: &Path) -> Result<(), Error> {
        let text = serde_json::to_string_pretty(self).map_err(|source| Error::Io {
            what: "write the package plan".into(),
            source: source.into(),
        })?;
        std::fs::write(path, text).map_err(|source| Error::Io {
            what: format!("write {}", path.display()),
            source,
        })
    }

    /// The plan [`Plan::save`] wrote to `path`.
    pub(super) fn load(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Read {
            path: path.to_owned(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| Error::PackagePlan {
            path: path.to_owned(),
            reason: source.to_string(),
        })
    }
}

/// The first 16 hex digits of the SHA-256 of `lines`, each ended by a
/// newline.
fn hash<S: AsRef<str>>(lines: impl Iterator<Item = S>) -> String {
    let mut sha = Sha256::new();
    for line in lines {
        sha.update(line.as_ref().as_bytes());
        sha.update(b"\n");
    }
    hex(&sha.finalize()[..8])
}

/// A size in megabytes, as apt writes one: `53.3 MB`.
pub(super) fn megabytes(bytes: u64) -> String {
    let tenths = (bytes + 50_000) / 100_000;
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `apt-get install --print-uris -qq` printed for two of sway's
    /// packages, after a warning of apt's own.
    const LISTED: &str = "\
W: Target Packages (main/binary-amd64/Packages) is configured multiple times
'http://azure.archive.ubuntu.com/ubuntu/pool/universe/s/sway/sway_1.11-1_amd64v3.deb' sway_1.11-1_amd64v3.deb 412318 SHA512:9f2c
'http://azure.archive.ubuntu.com/ubuntu/pool/main/libx/libxkbcommon/libxkbcommon0_1%3a1.11.0-1_amd64v3.deb' libxkbcommon0_1%3a1.11.0-1_amd64v3.deb 143020 SHA512:41ab
";

    fn plan() -> Plan {
        Plan::new(
            SystemTime::UNIX_EPOCH,
            &["sway".into(), "dbus-daemon".into(), "sway".into()],
            downloads(LISTED).unwrap(),
        )
    }

    #[test]
    fn the_listing_reads_as_downloads() {
        assert_eq!(
            downloads(LISTED).unwrap(),
            [
                Download {
                    file: "sway_1.11-1_amd64v3.deb".into(),
                    size: 412_318,
                    checksum: "SHA512:9f2c".into(),
                },
                Download {
                    file: "libxkbcommon0_1%3a1.11.0-1_amd64v3.deb".into(),
                    size: 143_020,
                    checksum: "SHA512:41ab".into(),
                },
            ]
        );
    }

    #[test]
    fn a_quoted_line_that_is_not_a_download_is_an_error() {
        for line in [
            "'http://host/a.deb' a.deb",
            "'http://host/a.deb' a.deb many SHA512:00",
            "'http://host/a.deb' a.deb 10 SHA512:00 more",
        ] {
            assert!(
                matches!(downloads(line), Err(Error::AptListing { .. })),
                "{line}"
            );
        }
    }

    #[test]
    fn the_plan_is_in_order_and_each_package_once() {
        let plan = plan();
        assert_eq!(plan.packages, ["dbus-daemon", "sway"]);
        assert_eq!(
            plan.downloads[0].file,
            "libxkbcommon0_1%3a1.11.0-1_amd64v3.deb"
        );
        assert_eq!(plan.bytes(), 555_338);
        assert_eq!(megabytes(plan.bytes()), "0.6 MB");
        assert_eq!(megabytes(53_349_999), "53.3 MB");
        assert_eq!(megabytes(53_350_000), "53.4 MB");
        assert_eq!(megabytes(0), "0.0 MB");
    }

    #[test]
    fn the_key_is_image_architecture_packages_and_files() {
        let plan = plan();
        let key = plan.key("ubuntu26", "X64");
        assert!(key.exact.starts_with(&key.prefix), "{key:?}");
        assert!(key.prefix.starts_with("apt-ubuntu26-X64-"), "{key:?}");
        let parts: Vec<&str> = key.exact.split('-').collect();
        assert_eq!(parts.len(), 5, "{key:?}");
        assert!(parts[3..].iter().all(|part| part.len() == 16), "{key:?}");
        // The order packages are named in does not change it.
        // Nor does when the step started.
        let again = Plan::new(
            SystemTime::now(),
            &["dbus-daemon".into(), "sway".into()],
            plan.downloads.clone(),
        );
        assert_eq!(again.key("ubuntu26", "X64"), key);
    }

    #[test]
    fn a_changed_file_changes_the_key_but_not_its_prefix() {
        let plan = plan();
        let key = plan.key("ubuntu26", "X64");
        for change in [
            |d: &mut Download| d.size += 1,
            |d: &mut Download| d.checksum.push('0'),
            |d: &mut Download| d.file.push('x'),
        ] {
            let mut changed = plan.clone();
            change(&mut changed.downloads[1]);
            let new = changed.key("ubuntu26", "X64");
            assert_ne!(new.exact, key.exact);
            assert_eq!(new.prefix, key.prefix);
        }
        let other = Plan::new(
            SystemTime::UNIX_EPOCH,
            &["labwc".into()],
            plan.downloads.clone(),
        );
        assert_ne!(other.key("ubuntu26", "X64").prefix, key.prefix);
        assert_ne!(plan.key("ubuntu24", "X64").prefix, key.prefix);
        assert_ne!(plan.key("ubuntu26", "ARM64").prefix, key.prefix);
    }

    #[test]
    fn a_saved_plan_loads_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.json");
        plan().save(&path).unwrap();
        assert_eq!(Plan::load(&path).unwrap(), plan());
        std::fs::write(&path, "{}").unwrap();
        assert!(matches!(Plan::load(&path), Err(Error::PackagePlan { .. })));
    }
}
