//! The shell's components, built for use: every one of
//! [`deskrinch_shell::packages`], in one `cargo build` of their own, with
//! the profile as the only choice. One home for that build, which `cargo
//! xtask desktop`, `cargo xtask compositor-tests`, the compositor tests'
//! bundle for CI (`cargo xtask bundle build`) and `cargo xtask
//! build-components` all use. A desktop can also be given them, built
//! elsewhere ([`Built::given`]).
//!
//! One invocation for all of them, always the same one: features unify over
//! the packages one invocation builds, so a build of some of them could give
//! a component another unit than a build of all, and which one a starter ran
//! would depend on what else it built. Not with anything else either: a
//! build together with the tests gives the components the test crates'
//! dev-dependency features, which nobody builds a component with for use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use crate::cargo_messages::{self, Kind};

/// How the components are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Profile {
    /// Cargo's `dev` profile: unoptimised.
    Dev,
    /// Cargo's `release` profile.
    Release,
}

/// Why the components could not be built.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BuildError {
    /// `cargo` could not be started.
    #[error("could not run cargo to build the shell's components")]
    Spawn(#[source] std::io::Error),
    /// `cargo` failed.
    #[error("building the shell's components failed ({status})")]
    Failed {
        /// How `cargo` exited.
        status: ExitStatus,
    },
    /// `cargo` built them but did not report one's binary in a target
    /// directory (`<target>/<profile>/<name>`).
    #[error("cargo reported no binary for {package}")]
    Missing {
        /// The package.
        package: &'static str,
    },
    /// A `--component` that is not `PACKAGE=PATH`.
    #[error("--component {0}: give PACKAGE=PATH, PACKAGE one of the shell's components")]
    NotUnderstood(String),
    /// A component given twice.
    #[error("{package} is given twice")]
    GivenTwice {
        /// The package.
        package: &'static str,
    },
    /// A component not given, when the others are.
    #[error("{package} is not given: give every component, or none to build them")]
    NotGiven {
        /// The package.
        package: &'static str,
    },
    /// A binary was asked for that is not one of the components'.
    #[error("{package} is none of the shell's components ({packages})")]
    Unknown {
        /// What was asked for.
        package: String,
        /// The components' packages.
        packages: String,
    },
}

/// What [`build`] built: every one of [`packages`]' binaries, and the
/// target directory Cargo put them in.
#[derive(Debug, Clone)]
pub(crate) struct Build {
    /// The binaries.
    pub(crate) built: Built,
    /// The target directory (they are in its `<profile>/`).
    pub(crate) target: PathBuf,
}

/// Every one of [`packages`]' binaries: built ([`build`]), or given
/// ([`Built::given`]).
#[derive(Debug, Clone)]
pub(crate) struct Built {
    binaries: BTreeMap<&'static str, PathBuf>,
}

impl Built {
    /// `package`'s binary.
    ///
    /// # Errors
    ///
    /// [`BuildError::Unknown`] when `package` is none of [`packages`].
    pub(crate) fn binary(&self, package: &str) -> Result<&Path, BuildError> {
        self.binaries
            .get(package)
            .map(PathBuf::as_path)
            .ok_or_else(|| unknown(package))
    }

    /// Every package and its binary.
    pub(crate) fn binaries(&self) -> impl Iterator<Item = (&'static str, &Path)> {
        self.binaries
            .iter()
            .map(|(&package, binary)| (package, binary.as_path()))
    }

    /// The binaries `asked` gives, each `PACKAGE=PATH`: every one of
    /// [`packages`], each once, built elsewhere (`cargo xtask desktop
    /// --component`).
    ///
    /// # Errors
    ///
    /// When one is not `PACKAGE=PATH`, names none of [`packages`], is given
    /// twice, or one of [`packages`] is not given.
    pub(crate) fn given(asked: &[String]) -> Result<Self, BuildError> {
        let mut binaries = BTreeMap::new();
        for text in asked {
            let (package, path) = text
                .split_once('=')
                .filter(|(package, path)| !package.is_empty() && !path.is_empty())
                .ok_or_else(|| BuildError::NotUnderstood(text.clone()))?;
            let package = packages()
                .into_iter()
                .find(|known| *known == package)
                .ok_or_else(|| unknown(package))?;
            if binaries.insert(package, PathBuf::from(path)).is_some() {
                return Err(BuildError::GivenTwice { package });
            }
        }
        if let Some(package) = packages()
            .into_iter()
            .find(|package| !binaries.contains_key(package))
        {
            return Err(BuildError::NotGiven { package });
        }
        Ok(Self { binaries })
    }

    /// Every package's binary as if built under `target`'s `debug`, for
    /// tests of what uses it.
    #[cfg(test)]
    pub(crate) fn at(target: &Path) -> Self {
        let binaries = packages()
            .into_iter()
            .map(|package| (package, target.join("debug").join(package)))
            .collect();
        Self { binaries }
    }
}

/// The error for `package`, none of [`packages`].
fn unknown(package: &str) -> BuildError {
    BuildError::Unknown {
        package: package.to_owned(),
        packages: packages().join(", "),
    }
}

/// The packages whose binaries the shell runs ([`deskrinch_shell::packages`]).
pub(crate) fn packages() -> Vec<&'static str> {
    deskrinch_shell::packages()
}

/// The arguments of the `cargo` invocation that builds [`packages`] with
/// `profile`: the one place they are spelt.
fn cargo_args(profile: Profile) -> Vec<&'static str> {
    let mut args = vec!["build"];
    for package in packages() {
        args.extend(["--package", package, "--bin", package]);
    }
    if profile == Profile::Release {
        args.push("--release");
    }
    args.extend(["--locked", "--message-format=json-render-diagnostics"]);
    args
}

/// Builds [`packages`] with `profile` in the workspace at `root`, Cargo's
/// progress on standard error; returns each package's binary, where Cargo
/// put it.
pub(crate) fn build(root: &Path, profile: Profile) -> Result<Build, BuildError> {
    let output = crate::cargo::command()
        .current_dir(root)
        .args(cargo_args(profile))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(BuildError::Spawn)?;
    if !output.status.success() {
        return Err(BuildError::Failed {
            status: output.status,
        });
    }
    let messages = String::from_utf8_lossy(&output.stdout);
    let mut binaries = BTreeMap::new();
    // One build's binaries are all in one target directory: each one's says
    // which.
    let mut target = PathBuf::new();
    for package in packages() {
        let binary = cargo_messages::executable(&messages, package, package, Kind::Bin)
            .ok_or(BuildError::Missing { package })?;
        // `<target>/<profile>/<name>`.
        binary
            .parent()
            .and_then(Path::parent)
            .ok_or(BuildError::Missing { package })?
            .clone_into(&mut target);
        binaries.insert(package, binary);
    }
    Ok(Build {
        built: Built { binaries },
        target,
    })
}

/// `cargo xtask build-components`: builds them all, and prints `package`'s
/// binary's path alone, or without one, each package and its binary's path,
/// a tab between, one a line. A `package` none of the components' is an
/// error before anything is built.
pub(crate) fn run(
    root: &Path,
    release: bool,
    package: Option<&str>,
) -> Result<crate::Outcome, crate::error::Error> {
    if let Some(package) = package
        && !packages().contains(&package)
    {
        return Err(unknown(package).into());
    }
    let profile = if release {
        Profile::Release
    } else {
        Profile::Dev
    };
    let built = build(root, profile)?.built;
    match package {
        Some(package) => println!("{}", built.binary(package)?.display()),
        None => {
            for (package, binary) in built.binaries() {
                println!("{package}\t{}", binary.display());
            }
        }
    }
    Ok(crate::Outcome::Passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_given_are_every_one_each_once() {
        let every: Vec<String> = packages()
            .iter()
            .map(|package| format!("{package}=/b/{package}"))
            .collect();
        let given = Built::given(&every).unwrap();
        assert_eq!(
            given.binary("deskrinch-panel").unwrap(),
            Path::new("/b/deskrinch-panel")
        );
        assert!(matches!(
            Built::given(&every[1..]),
            Err(BuildError::NotGiven { .. })
        ));
        let twice = [every.clone(), vec![every[0].clone()]].concat();
        assert!(matches!(
            Built::given(&twice),
            Err(BuildError::GivenTwice { .. })
        ));
        for wrong in ["deskrinch-panel", "=/b/x", "deskrinch-panel="] {
            let asked = [vec![wrong.to_owned()], every.clone()].concat();
            assert!(
                matches!(Built::given(&asked), Err(BuildError::NotUnderstood(_))),
                "{wrong}"
            );
        }
        let unknown = [vec!["deskrinch-nothing=/b/x".to_owned()], every].concat();
        assert!(matches!(
            Built::given(&unknown),
            Err(BuildError::Unknown { .. })
        ));
    }

    #[test]
    fn the_invocation_is_this_one_for_each_profile() {
        let mut dev = vec!["build"];
        for package in packages() {
            dev.extend(["--package", package, "--bin", package]);
        }
        let mut release = dev.clone();
        release.push("--release");
        for args in [&mut dev, &mut release] {
            args.extend(["--locked", "--message-format=json-render-diagnostics"]);
        }
        assert_eq!(cargo_args(Profile::Dev), dev);
        assert_eq!(cargo_args(Profile::Release), release);
        assert_eq!(
            cargo_args(Profile::Dev)[..9],
            [
                "build",
                "--package",
                "deskrinch-wallpaper",
                "--bin",
                "deskrinch-wallpaper",
                "--package",
                "deskrinch-notifications",
                "--bin",
                "deskrinch-notifications",
            ],
            "the components in the order they start"
        );
        assert!(!cargo_args(Profile::Dev).contains(&"--features"));
    }
}
