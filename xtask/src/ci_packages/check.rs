//! What of a restored cache apt may install from: each `.deb` file checked
//! against the plan, by our own reading of its bytes.
//!
//! The cache is restored by the job's user, and apt installs as root what
//! it finds in its archives directory. So before apt runs, every file there
//! is held to the size and checksum apt's signed index gave for it (in the
//! plan, from `--print-uris`), and a file that does not match, or that the
//! plan does not name, is removed for apt to fetch. A checksum of a kind
//! other than SHA-256 or SHA-512 is not verified, so its file is removed
//! too. What root installs from the cache is then exactly what the index
//! names.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::fs::{self, File};
use std::io::Read as _;
use std::path::Path;

use sha2::{Digest, Sha256, Sha512};

use super::cache::contents;
use super::plan::Download;
use crate::error::Error;

/// Why a restored file is not installed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Unfit {
    /// apt no longer fetches it: an older cache's.
    NotPlanned,
    /// Its size is not the index's.
    Size {
        /// Its size, in bytes.
        found: u64,
        /// The index's.
        wanted: u64,
    },
    /// Its checksum is not the index's.
    Checksum {
        /// The kind of checksum, such as `SHA256`.
        kind: String,
    },
    /// The index gives a checksum of a kind this does not verify.
    Unverified {
        /// The kind, as apt names it.
        kind: String,
    },
}

impl fmt::Display for Unfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPlanned => f.write_str("apt no longer fetches it"),
            Self::Size { found, wanted } => {
                write!(f, "{found} bytes, where the index says {wanted}")
            }
            Self::Checksum { kind } => write!(f, "its {kind} is not the index's"),
            Self::Unverified { kind } => write!(f, "its {kind} checksum is one not verified here"),
        }
    }
}

/// The restored cache, checked.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Checked {
    /// The files that match the plan, each with its size.
    pub(super) kept: BTreeMap<String, u64>,
    /// The files removed, each with why.
    pub(super) removed: Vec<(String, Unfit)>,
}

/// Checks each `.deb` file in `dir` against `downloads`, the plan's, and
/// removes those that do not match. apt's `lock` and `partial` are not
/// touched.
pub(super) fn check(dir: &Path, downloads: &[Download]) -> Result<Checked, Error> {
    let planned: BTreeMap<&str, &Download> =
        downloads.iter().map(|d| (d.file.as_str(), d)).collect();
    let mut checked = Checked::default();
    for (file, size) in contents(dir)? {
        let path = dir.join(&file);
        let unfit = match planned.get(file.as_str()) {
            None => Some(Unfit::NotPlanned),
            Some(download) => fits(&path, size, download)?,
        };
        match unfit {
            None => {
                checked.kept.insert(file, size);
            }
            Some(unfit) => {
                fs::remove_file(&path).map_err(|source| Error::Io {
                    what: format!("remove {}", path.display()),
                    source,
                })?;
                checked.removed.push((file, unfit));
            }
        }
    }
    Ok(checked)
}

/// Whether the file at `path`, `size` bytes, is `download`: `None` if it
/// is, else why not.
fn fits(path: &Path, size: u64, download: &Download) -> Result<Option<Unfit>, Error> {
    if size != download.size {
        return Ok(Some(Unfit::Size {
            found: size,
            wanted: download.size,
        }));
    }
    let (kind, wanted) = download
        .checksum
        .split_once(':')
        .unwrap_or((download.checksum.as_str(), ""));
    let found = match kind {
        "SHA256" => digest::<Sha256>(path)?,
        "SHA512" => digest::<Sha512>(path)?,
        _ => {
            return Ok(Some(Unfit::Unverified {
                kind: kind.to_owned(),
            }));
        }
    };
    Ok(
        (!found.eq_ignore_ascii_case(wanted)).then(|| Unfit::Checksum {
            kind: kind.to_owned(),
        }),
    )
}

/// The hex digest `D` of the file at `path`.
fn digest<D: Digest>(path: &Path) -> Result<String, Error> {
    let io = |source| Error::Read {
        path: path.to_owned(),
        source,
    };
    let mut file = File::open(path).map_err(io)?;
    let mut hasher = D::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// `bytes` in lower-case hex.
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _infallible = write!(hex, "{byte:02x}");
        hex
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package's bytes, as the index describes them with `kind`.
    fn package(file: &str, bytes: &[u8], kind: &str) -> Download {
        let checksum = match kind {
            "SHA256" => format!("SHA256:{}", hex(&Sha256::digest(bytes))),
            "SHA512" => format!("SHA512:{}", hex(&Sha512::digest(bytes))),
            other => format!("{other}:00"),
        };
        Download {
            file: file.into(),
            size: bytes.len() as u64,
            checksum,
        }
    }

    /// A restored directory holding `files`, with apt's lock and a partial
    /// download beside them.
    fn restored(files: &[(&str, &[u8])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (file, bytes) in files {
            fs::write(dir.path().join(file), bytes).unwrap();
        }
        fs::write(dir.path().join("lock"), []).unwrap();
        fs::create_dir(dir.path().join("partial")).unwrap();
        fs::write(dir.path().join("partial/labwc_0.9_amd64v3.deb"), [0; 50]).unwrap();
        dir
    }

    const GOOD: &[u8] = b"the package apt's index describes";

    #[test]
    fn a_file_the_index_describes_is_kept() {
        for kind in ["SHA256", "SHA512"] {
            let dir = restored(&[("sway.deb", GOOD)]);
            let checked = check(dir.path(), &[package("sway.deb", GOOD, kind)]).unwrap();
            assert_eq!(checked.removed, [], "{kind}");
            assert_eq!(
                checked.kept.into_iter().collect::<Vec<_>>(),
                [("sway.deb".into(), 33)]
            );
        }
    }

    /// The right name and size are not enough.
    #[test]
    fn other_bytes_of_the_same_size_are_removed() {
        let other = b"the package someone else describes";
        let other = &other[..GOOD.len()];
        let dir = restored(&[("sway.deb", other)]);
        let checked = check(dir.path(), &[package("sway.deb", GOOD, "SHA512")]).unwrap();
        assert_eq!(
            checked.removed,
            [(
                "sway.deb".into(),
                Unfit::Checksum {
                    kind: "SHA512".into()
                }
            )]
        );
        assert!(checked.kept.is_empty());
        assert!(!dir.path().join("sway.deb").exists());
    }

    #[test]
    fn a_truncated_file_is_removed() {
        let dir = restored(&[("sway.deb", &GOOD[..10])]);
        let checked = check(dir.path(), &[package("sway.deb", GOOD, "SHA256")]).unwrap();
        assert_eq!(
            checked.removed,
            [(
                "sway.deb".into(),
                Unfit::Size {
                    found: 10,
                    wanted: 33
                }
            )]
        );
        assert!(!dir.path().join("sway.deb").exists());
    }

    #[test]
    fn a_checksum_of_a_kind_not_verified_removes_the_file() {
        let dir = restored(&[("cowsay.deb", GOOD)]);
        let checked = check(dir.path(), &[package("cowsay.deb", GOOD, "MD5Sum")]).unwrap();
        assert_eq!(
            checked.removed,
            [(
                "cowsay.deb".into(),
                Unfit::Unverified {
                    kind: "MD5Sum".into()
                }
            )]
        );
        assert!(!dir.path().join("cowsay.deb").exists());
    }

    #[test]
    fn a_file_apt_no_longer_fetches_is_removed_and_apts_own_files_are_left() {
        let dir = restored(&[("sway_1.10.deb", GOOD), ("sway_1.11.deb", GOOD)]);
        let checked = check(dir.path(), &[package("sway_1.11.deb", GOOD, "SHA512")]).unwrap();
        assert_eq!(
            checked.removed,
            [("sway_1.10.deb".into(), Unfit::NotPlanned)]
        );
        assert_eq!(checked.kept.len(), 1);
        assert!(dir.path().join("lock").exists());
        assert!(dir.path().join("partial/labwc_0.9_amd64v3.deb").exists());
    }

    #[test]
    fn each_reason_reads() {
        assert_eq!(Unfit::NotPlanned.to_string(), "apt no longer fetches it");
        assert_eq!(
            Unfit::Size {
                found: 10,
                wanted: 33
            }
            .to_string(),
            "10 bytes, where the index says 33"
        );
        assert_eq!(
            Unfit::Checksum {
                kind: "SHA512".into()
            }
            .to_string(),
            "its SHA512 is not the index's"
        );
    }
}
