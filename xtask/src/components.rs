//! The shell's components, built for use: every one of
//! `deskrinch_shell::COMPONENTS` and what the key bindings run, in one
//! `cargo build` of their own, with the profile as the only choice. One
//! home for that build, which `cargo xtask desktop`, `cargo xtask
//! compositor-tests` and `cargo xtask build-components` (CI's measure steps)
//! all use.
//!
//! One invocation for all of them, always the same one: features unify over
//! the packages one invocation builds, so a build of some of them could give
//! a component another unit than a build of all, and which one a starter ran
//! would depend on what else it built. Not with anything else either: a
//! build together with the tests gives the components the test crates'
//! dev-dependency features, which nobody builds a component with for use.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use deskrinch_shell::{COMPONENTS, LAUNCHER_TOGGLE};

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
    /// A binary was asked for that is not one of the components'.
    #[error("{package} is none of the shell's components ({packages})")]
    Unknown {
        /// What was asked for.
        package: String,
        /// The components' packages.
        packages: String,
    },
}

/// What [`build`] built: every one of [`packages`]' binaries, and the target
/// directory Cargo put them in.
#[derive(Debug, Clone)]
pub(crate) struct Built {
    binaries: BTreeMap<&'static str, PathBuf>,
    target: PathBuf,
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

    /// The target directory Cargo put the binaries in (they are in its
    /// `<profile>/`).
    pub(crate) fn target_dir(&self) -> &Path {
        &self.target
    }

    /// Every package's binary as if built under `target`'s `debug`, for
    /// tests of what uses it.
    #[cfg(test)]
    pub(crate) fn at(target: &Path) -> Self {
        let binaries = packages()
            .into_iter()
            .map(|package| (package, target.join("debug").join(package)))
            .collect();
        Self {
            binaries,
            target: target.to_owned(),
        }
    }
}

/// The error for `package`, none of [`packages`].
fn unknown(package: &str) -> BuildError {
    BuildError::Unknown {
        package: package.to_owned(),
        packages: packages().join(", "),
    }
}

/// The packages whose binaries the shell runs: the components, and what the
/// key bindings run, each once, in that order.
pub(crate) fn packages() -> Vec<&'static str> {
    let mut packages: Vec<&'static str> = COMPONENTS.iter().map(|c| c.package).collect();
    if !packages.contains(&LAUNCHER_TOGGLE.package) {
        packages.push(LAUNCHER_TOGGLE.package);
    }
    packages
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
pub(crate) fn build(root: &Path, profile: Profile) -> Result<Built, BuildError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(&cargo)
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
    Ok(Built { binaries, target })
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
    let built = build(root, profile)?;
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
    fn builds_every_component_and_what_the_bindings_run_once() {
        let packages = packages();
        for component in COMPONENTS {
            assert_eq!(
                packages.iter().filter(|p| **p == component.package).count(),
                1,
                "{}",
                component.package
            );
        }
        assert!(packages.contains(&LAUNCHER_TOGGLE.package));
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
