//! The compositor tests' bundle: everything the suites and the session
//! tests run, built once ([`Bundle::build`]) and carried to CI's shard jobs
//! ([`super::shards`]) as one archive, so that a shard runs suites and
//! builds nothing.
//!
//! It holds every suite's test binary, the components built for use, the
//! session tests' binaries and xtask's own binary (the one the tests were
//! built with), and a manifest ([`manifest`]). It lives in
//! [`DIRECTORY`] under the workspace root, and the archive holds it with
//! the files that must sit where they were built: the tests run xtask by
//! the path compiled into them (`CARGO_BIN_EXE_xtask`), so the archive is
//! unpacked at the workspace root, and [`Bundle::open`] fails unless that
//! root is the one the bundle was built in and those files are the ones it
//! was built with.

mod check;
mod make;
mod manifest;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::Sha256;

use self::manifest::Manifest;
pub(crate) use self::manifest::{BundleError, Image};
use super::build::Target;
use crate::Outcome;
use crate::components;
use crate::error::Error;
use crate::session_tests::SESSION_TESTS;

/// Where the bundle lives, under the workspace root.
pub(crate) const DIRECTORY: &str = "target/compositor-bundle";
/// Its manifest's name in [`DIRECTORY`].
const MANIFEST: &str = "manifest.json";

/// A bundle, opened and checked.
#[derive(Debug)]
pub(crate) struct Bundle {
    root: PathBuf,
    manifest: Manifest,
}

/// `cargo xtask bundle`'s steps.
#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand)]
pub(crate) enum Command {
    /// Build everything the compositor tests and the session tests run,
    /// into the bundle (`target/compositor-bundle`), and pack it with the
    /// files it needs at their build paths into ARCHIVE, a tar.zst to
    /// unpack at the workspace root.
    Build {
        /// The archive to write.
        archive: PathBuf,
    },
    /// Check the unpacked bundle: that it was built in this checkout, that
    /// its files are in place, that every library its binaries need is
    /// here; and say whether it was built on this runner image.
    Check,
    /// Print the path of PACKAGE's binary in the bundle: a component.
    Component {
        /// The component's package.
        package: String,
    },
}

/// Runs `command` in the workspace at `root`.
pub(crate) fn run(root: &Path, command: &Command) -> Result<Outcome, Error> {
    match command {
        Command::Build { archive } => {
            let bundle = Bundle::build(root)?;
            make::archive(&bundle, archive)?;
            Ok(Outcome::Passed)
        }
        Command::Check => {
            let bundle = Bundle::open(root)?;
            println!(
                "{}",
                check::images(bundle.manifest.image.as_ref(), Image::here(env))
            );
            check::libraries(&bundle.files())?;
            println!("bundle: every library its binaries need is here");
            Ok(Outcome::Passed)
        }
        Command::Component { package } => {
            let bundle = Bundle::open(root)?;
            println!("{}", bundle.component(package)?.display());
            Ok(Outcome::Passed)
        }
    }
}

/// The environment variable `name`, if set and Unicode.
fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

impl Bundle {
    /// The bundle unpacked under `root`, checked: built in this root, with
    /// every file it names in place and executable, and each file that must
    /// be at its build path the one it was built with.
    pub(crate) fn open(root: &Path) -> Result<Self, Error> {
        let root = canonical(root)?;
        let path = root.join(DIRECTORY).join(MANIFEST);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BundleError::NotUnpacked {
                    dir: root.join(DIRECTORY),
                }
                .into());
            }
            Err(source) => return Err(Error::Read { path, source }),
        };
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|source| BundleError::Manifest { path, source })?;
        if manifest.root != root {
            return Err(BundleError::Root {
                built: manifest.root,
                here: root,
            }
            .into());
        }
        let bundle = Self { root, manifest };
        for (path, sha) in &bundle.manifest.baked {
            let path = bundle.root.join(path);
            let found = crate::digest::of_file::<Sha256>(&path).ok();
            if found.as_ref() != Some(sha) {
                return Err(BundleError::NotAsBuilt { path }.into());
            }
        }
        for path in bundle.files() {
            if !executable(&path) {
                return Err(BundleError::Missing { path }.into());
            }
        }
        Ok(bundle)
    }

    /// Every binary it holds, those at their build paths included.
    fn files(&self) -> Vec<PathBuf> {
        let manifest = &self.manifest;
        let held = manifest
            .tests
            .values()
            .chain(manifest.components.values())
            .chain(manifest.session_tests.values())
            .map(|path| self.dir().join(path));
        let baked = manifest.baked.keys().map(|path| self.root.join(path));
        held.chain(baked).collect()
    }

    /// Its directory.
    fn dir(&self) -> PathBuf {
        self.root.join(DIRECTORY)
    }

    /// `target`'s test binary.
    pub(crate) fn test(&self, target: Target) -> Result<PathBuf, BundleError> {
        self.manifest
            .tests
            .get(&target.name())
            .map(|path| self.dir().join(path))
            .ok_or_else(|| not_held(format!("test binary for {}", target.name())))
    }

    /// The binaries granted with `target`'s suite, by `[[bin]]` name.
    pub(crate) fn granted(
        &self,
        target: Target,
    ) -> Result<Vec<(&'static str, PathBuf)>, BundleError> {
        let held = self.manifest.granted.get(&target.name());
        super::granted::with(target)
            .map(|bin| {
                held.and_then(|bins| bins.get(bin))
                    .map(|path| (bin, self.root.join(path)))
                    .ok_or_else(|| not_held(format!("{bin} granted with {}", target.name())))
            })
            .collect()
    }

    /// `package`'s component binary.
    pub(crate) fn component(&self, package: &str) -> Result<PathBuf, Error> {
        if !components::packages().contains(&package) {
            return Err(components::BuildError::Unknown {
                package: package.to_owned(),
                packages: components::packages().join(", "),
            }
            .into());
        }
        self.manifest
            .components
            .get(package)
            .map(|path| self.dir().join(path))
            .ok_or_else(|| not_held(format!("component {package}")).into())
    }

    /// Every component's binary, by package.
    pub(crate) fn components(&self) -> Result<BTreeMap<&'static str, PathBuf>, Error> {
        components::packages()
            .into_iter()
            .map(|package| Ok((package, self.component(package)?)))
            .collect()
    }

    /// Each session test's binary, by name, in [`SESSION_TESTS`]' order.
    pub(crate) fn session_tests(&self) -> Result<Vec<(&'static str, PathBuf)>, BundleError> {
        SESSION_TESTS
            .iter()
            .map(|test| {
                self.manifest
                    .session_tests
                    .get(test.name)
                    .map(|path| (test.name, self.dir().join(path)))
                    .ok_or_else(|| not_held(format!("session test {}", test.name)))
            })
            .collect()
    }
}

/// The error for `what`, which the manifest does not name.
fn not_held(what: String) -> BundleError {
    BundleError::NotHeld { what }
}

/// `root`, its links resolved.
fn canonical(root: &Path) -> Result<PathBuf, Error> {
    root.canonicalize().map_err(|source| Error::Read {
        path: root.to_owned(),
        source,
    })
}

/// Whether `path` is a regular file someone may execute.
fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests;
