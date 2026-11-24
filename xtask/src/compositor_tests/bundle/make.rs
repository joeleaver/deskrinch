//! Making the bundle: one build of what the suites and the session tests
//! run, linked or copied into [`DIRECTORY`], and the archive of it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use sha2::Sha256;

use super::manifest::Manifest;
use super::{Bundle, BundleError, DIRECTORY, Image, MANIFEST, canonical, env};
use crate::components::{self, Profile};
use crate::compositor_tests::build::{Target, test_binaries};
use crate::compositor_tests::suites::{INPUT_CLIENT, SUITES};
use crate::error::Error;
use crate::session_tests::SESSION_TESTS;

/// Every target the bundle builds, each once: the suites' and the session
/// tests', in the table's order. One `cargo test` per backend builds them
/// (`test_binaries`), as `cargo xtask compositor-tests` builds the suites,
/// so xtask's binary is one unit for every test that runs it.
fn targets() -> Vec<Target> {
    let mut targets: Vec<Target> = Vec::new();
    let all = SUITES
        .iter()
        .map(|suite| suite.target)
        .chain(SESSION_TESTS.iter().map(|test| test.target()));
    for target in all {
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

impl Bundle {
    /// Builds every suite's and session test's binary, then the components
    /// as they are built for use, and puts them in the bundle under `root`
    /// with its manifest.
    pub(crate) fn build(root: &Path) -> Result<Self, Error> {
        let root = canonical(root)?;
        let targets = targets();
        let built = test_binaries(&root, &targets)?;
        let components = components::build(&root, Profile::Dev)?.built;
        let dir = root.join(DIRECTORY);
        let io = |what: &str| {
            let what = format!("{what} in {}", dir.display());
            move |source| Error::Io { what, source }
        };
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(io("empty the bundle"))?;
        }
        let mut manifest = Manifest {
            root: root.clone(),
            image: Image::here(env),
            baked: BTreeMap::new(),
            xtask: PathBuf::new(),
            tests: BTreeMap::new(),
            granted: BTreeMap::new(),
            components: BTreeMap::new(),
            session_tests: BTreeMap::new(),
        };
        for (target, binaries) in targets.iter().zip(&built) {
            let session = SESSION_TESTS.iter().find(|test| test.target() == *target);
            let held = match session {
                Some(test) => PathBuf::from("session-tests").join(test.name),
                None => PathBuf::from("tests").join(file_name(&binaries.test)),
            };
            keep(&binaries.test, &dir.join(&held)).map_err(io("keep a test binary"))?;
            match session {
                Some(test) => manifest.session_tests.insert(test.name.to_owned(), held),
                None => manifest.tests.insert(target.name(), held),
            };
            for (bin, path) in &binaries.granted {
                let baked = baked(&root, path)?;
                manifest
                    .granted
                    .entry(target.name())
                    .or_default()
                    .insert((*bin).to_owned(), baked.clone());
                if *target == INPUT_CLIENT && *bin == "xtask" {
                    manifest.xtask = baked;
                }
            }
        }
        if manifest.xtask.as_os_str().is_empty() {
            return Err(BundleError::NotHeld {
                what: "xtask binary (granted with xtask's input client test)".into(),
            }
            .into());
        }
        for (package, binary) in components.binaries() {
            let held = PathBuf::from("components").join(package);
            keep(binary, &dir.join(&held)).map_err(io("keep a component"))?;
            manifest.components.insert(package.to_owned(), held);
        }
        let baked: Vec<PathBuf> = manifest
            .granted
            .values()
            .flat_map(|bins| bins.values().cloned())
            .collect();
        for path in baked {
            let sha = crate::digest::of_file::<Sha256>(&root.join(&path))?;
            manifest.baked.insert(path, sha);
        }
        let text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
        fs::write(dir.join(MANIFEST), text).map_err(io("write the manifest"))?;
        Ok(Self { root, manifest })
    }
}

/// `path`, which must stay where it was built, relative to `root`.
fn baked(root: &Path, path: &Path) -> Result<PathBuf, BundleError> {
    path.strip_prefix(root)
        .map(Path::to_owned)
        .map_err(|_| BundleError::OutsideRoot {
            path: path.to_owned(),
            root: root.to_owned(),
        })
}

/// The file name of `path`, which Cargo makes unique per test target
/// (`compositor-<hash>`).
fn file_name(path: &Path) -> PathBuf {
    path.file_name().map(PathBuf::from).unwrap_or_default()
}

/// Puts `built` at `kept`, a hard link where it can be, else a copy: a
/// later build replaces Cargo's file, never the one kept.
fn keep(built: &Path, kept: &Path) -> std::io::Result<()> {
    if let Some(parent) = kept.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::hard_link(built, kept).is_err() {
        fs::copy(built, kept)?;
    }
    Ok(())
}

/// The archive's members, relative to the root: the bundle's directory and
/// every file at its build path.
fn members(bundle: &Bundle) -> Vec<PathBuf> {
    std::iter::once(PathBuf::from(DIRECTORY))
        .chain(bundle.manifest.baked.keys().cloned())
        .collect()
}

/// The command that packs `bundle` into `archive` with zstd, on every
/// core, at level 3: a higher level saves a little of each shard's download
/// for more of the build job's time, which every run waits for.
fn tar(bundle: &Bundle, archive: &Path) -> Command {
    let mut command = Command::new("tar");
    command
        .arg("--use-compress-program=zstd -T0 -3")
        .arg("-cf")
        .arg(archive)
        .arg("-C")
        .arg(&bundle.root)
        .args(members(bundle));
    command
}

/// Packs `bundle` into `archive`, a tar.zst to unpack at the workspace
/// root, says how large it is, and removes the bundle's directory, which
/// the archive now carries: a build cache saved after it holds none of it.
pub(super) fn archive(bundle: &Bundle, archive: &Path) -> Result<(), Error> {
    let mut command = tar(bundle, archive);
    let started = Instant::now();
    let status = command.status().map_err(|source| Error::Spawn {
        program: "tar".into(),
        source,
    })?;
    if !status.success() {
        return Err(BundleError::Archive {
            command: format!("{command:?}"),
            status,
        }
        .into());
    }
    let took = started.elapsed();
    let size = fs::metadata(archive)
        .map_err(|source| Error::Read {
            path: archive.to_owned(),
            source,
        })?
        .len();
    let raw: u64 = bundle
        .files()
        .iter()
        .filter_map(|file| fs::metadata(file).ok())
        .map(|meta| meta.len())
        .sum();
    println!(
        "bundle: {} binaries, {} MB, packed into {} ({} MB) in {:.1} s",
        bundle.files().len(),
        raw / 1_000_000,
        archive.display(),
        size / 1_000_000,
        took.as_secs_f64()
    );
    let dir = bundle.dir();
    fs::remove_dir_all(&dir).map_err(|source| Error::Io {
        what: format!("remove {} once packed", dir.display()),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_is_built_once_the_session_tests_with_the_suites() {
        let targets = targets();
        for (index, target) in targets.iter().enumerate() {
            assert!(!targets[index + 1..].contains(target), "{target:?} twice");
        }
        assert!(targets.contains(&INPUT_CLIENT));
        for test in SESSION_TESTS {
            assert!(targets.contains(&test.target()), "{}", test.name);
        }
        for suite in &SUITES {
            assert!(targets.contains(&suite.target));
        }
    }

    #[test]
    fn a_build_path_outside_the_root_is_refused() {
        let root = Path::new("/w");
        assert_eq!(
            baked(root, Path::new("/w/target/debug/xtask")).unwrap(),
            Path::new("target/debug/xtask")
        );
        assert!(matches!(
            baked(root, Path::new("/elsewhere/xtask")),
            Err(BundleError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn a_kept_file_survives_its_build_being_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let built = dir.path().join("built");
        fs::write(&built, b"first").unwrap();
        let kept = dir.path().join("bundle/tests/kept");
        keep(&built, &kept).unwrap();
        fs::remove_file(&built).unwrap();
        fs::write(&built, b"second").unwrap();
        assert_eq!(fs::read(&kept).unwrap(), b"first");
    }
}
