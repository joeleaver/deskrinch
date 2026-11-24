//! What a bundle holds, as `manifest.json` beside its binaries says, and
//! why one cannot be used.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The bundle's manifest. Paths in the bundle are relative to its
/// directory; paths at their build place ([`Manifest::baked`]) are
/// relative to the workspace root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Manifest {
    /// The workspace root it was built in, canonical: every compile-time
    /// path in its binaries is under it.
    pub(super) root: PathBuf,
    /// The runner image it was built on, when built on one.
    pub(super) image: Option<Image>,
    /// The files that must be at the path they were built at, by that path
    /// under the root, with their SHA-256: xtask's binary, which the tests
    /// run by the path compiled into them (`CARGO_BIN_EXE_xtask`), and the
    /// binaries granted with a suite, which KWin grants by their path.
    pub(super) baked: BTreeMap<PathBuf, String>,
    /// xtask's binary, the one the tests were built with: one of
    /// [`Manifest::baked`].
    pub(super) xtask: PathBuf,
    /// Each suite target's test binary, by the target's name.
    pub(super) tests: BTreeMap<String, PathBuf>,
    /// Each suite target's granted binaries ([`super::super::granted`]):
    /// `[[bin]]` name and path, one of [`Manifest::baked`].
    pub(super) granted: BTreeMap<String, BTreeMap<String, PathBuf>>,
    /// Each component's binary, by package.
    pub(super) components: BTreeMap<String, PathBuf>,
    /// Each session test's binary, by its `[[test]]` name.
    pub(super) session_tests: BTreeMap<String, PathBuf>,
}

/// A GitHub runner image: `ImageOS` and `ImageVersion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Image {
    pub(crate) os: String,
    pub(crate) version: String,
}

impl Image {
    /// The image this runs on, read from `var`; `None` off a runner.
    pub(crate) fn here(var: impl Fn(&str) -> Option<String>) -> Option<Self> {
        Some(Self {
            os: var("ImageOS")?,
            version: var("ImageVersion")?,
        })
    }
}

impl fmt::Display for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.os, self.version)
    }
}

/// Why a bundle cannot be made or used.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BundleError {
    /// There is no bundle: its archive was not unpacked here.
    #[error(
        "no bundle in {}: unpack the archive `cargo xtask bundle build` makes at the \
         workspace root",
        dir.display()
    )]
    NotUnpacked { dir: PathBuf },
    /// The manifest is not one this xtask reads.
    #[error("{} is not a bundle's manifest: {source}", path.display())]
    Manifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// It was built in another workspace root, so the paths compiled into
    /// its binaries point elsewhere.
    #[error(
        "the bundle was built in {} and this is {}: the paths its tests were \
         compiled with (CARGO_BIN_EXE_xtask) are not this checkout's",
        built.display(),
        here.display()
    )]
    Root { built: PathBuf, here: PathBuf },
    /// A file that must be at its build path is missing there, or is not
    /// the one built (unpack the archive at the workspace root).
    #[error(
        "{} is not the file the bundle was built with (unpack its archive at \
         the workspace root)",
        path.display()
    )]
    NotAsBuilt { path: PathBuf },
    /// A file the manifest names is missing, or not executable.
    #[error("the bundle's {} is missing or not executable", path.display())]
    Missing { path: PathBuf },
    /// The manifest names nothing for what a run needs.
    #[error("the bundle holds no {what}")]
    NotHeld { what: String },
    /// A build path outside the workspace root, which the archive cannot
    /// carry to its place.
    #[error("{} is outside the workspace root {}", path.display(), root.display())]
    OutsideRoot { path: PathBuf, root: PathBuf },
    /// Libraries the bundle's binaries need that this machine lacks.
    #[error("the bundle's binaries need libraries this machine lacks:\n{}", lines.join("\n"))]
    MissingLibraries { lines: Vec<String> },
    /// The archive could not be made or read.
    #[error("{command} failed ({status})")]
    Archive {
        command: String,
        status: std::process::ExitStatus,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_image_is_read_off_a_runner_only() {
        let runner = |name: &str| match name {
            "ImageOS" => Some("ubuntu26".to_owned()),
            "ImageVersion" => Some("20261117.1".to_owned()),
            _ => None,
        };
        let image = Image::here(runner).unwrap();
        assert_eq!(image.to_string(), "ubuntu26 20261117.1");
        assert_eq!(Image::here(|_| None), None);
    }

    #[test]
    fn a_manifest_reads_back_as_written() {
        let manifest = Manifest {
            root: "/w".into(),
            image: None,
            baked: BTreeMap::from([("target/debug/xtask".into(), "ab".to_owned())]),
            xtask: "target/debug/xtask".into(),
            tests: BTreeMap::from([("xtask desktop".to_owned(), "tests/desktop-1".into())]),
            granted: BTreeMap::new(),
            components: BTreeMap::new(),
            session_tests: BTreeMap::new(),
        };
        let text = serde_json::to_string(&manifest).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&text).unwrap(), manifest);
    }
}
