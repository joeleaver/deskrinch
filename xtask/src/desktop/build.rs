//! Building the shell's components, and finding where Cargo put them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use deskrinch_shell::{COMPONENTS, LAUNCHER_TOGGLE};

use super::DesktopError;
use crate::cargo_messages::{self, Kind};
use crate::error::Error;

/// The packages whose binaries the desktop runs: the components, and what
/// the key bindings run, each once.
pub(super) fn packages() -> Vec<&'static str> {
    let mut packages: Vec<&'static str> = COMPONENTS.iter().map(|c| c.package).collect();
    if !packages.contains(&LAUNCHER_TOGGLE.package) {
        packages.push(LAUNCHER_TOGGLE.package);
    }
    packages
}

/// Builds [`packages`] in the workspace at `root`, optimised unless
/// `debug`, saying what it builds; returns each package's binary.
pub(super) fn binaries(root: &Path, debug: bool) -> Result<BTreeMap<&'static str, PathBuf>, Error> {
    let packages = packages();
    eprintln!(
        "desktop: building {} ({})",
        packages.join(", "),
        if debug {
            "unoptimised, as --debug asks"
        } else {
            "release; --debug builds them unoptimised"
        }
    );
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(&cargo);
    command.current_dir(root).arg("build");
    for package in &packages {
        command.args(["--package", package, "--bin", package]);
    }
    if !debug {
        command.arg("--release");
    }
    let output = command
        .args(["--locked", "--message-format=json-render-diagnostics"])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|source| Error::Spawn {
            program: "cargo".into(),
            source,
        })?;
    if !output.status.success() {
        return Err(DesktopError::BuildFailed {
            status: output.status,
        }
        .into());
    }
    let messages = String::from_utf8_lossy(&output.stdout);
    packages
        .into_iter()
        .map(|package| {
            cargo_messages::executable(&messages, package, package, Kind::Bin)
                .map(|binary| (package, binary))
                .ok_or_else(|| DesktopError::MissingBinary { package }.into())
        })
        .collect()
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
                1
            );
        }
        assert!(packages.contains(&LAUNCHER_TOGGLE.package));
    }
}
