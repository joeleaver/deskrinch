//! Building the compositor test binaries and finding where Cargo put them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::Error;

/// Which implementation of the Wayland client library a test binary is
/// built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Backend {
    /// wayland-backend's own Rust implementation, the default.
    Rust,
    /// libwayland (`wayland-client`'s `system` feature), which every binary
    /// that links rinch gets through winit.
    System,
}

impl Backend {
    /// The features that select it, for `cargo test`.
    fn features(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[],
            Self::System => &["--features", "wayland-client/system"],
        }
    }
}

/// A test target of a package, by its `[[test]]` name, built with a
/// Wayland backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Target {
    /// The package.
    pub(super) package: &'static str,
    /// The `[[test]]` name.
    pub(super) test: &'static str,
    /// The Wayland client library's implementation.
    pub(super) backend: Backend,
}

/// Builds `targets`, one `cargo test --no-run` per backend, and returns
/// their binaries, in the same order.
pub(super) fn test_binaries(root: &Path, targets: &[Target]) -> Result<Vec<PathBuf>, Error> {
    let mut binaries: Vec<Option<PathBuf>> = vec![None; targets.len()];
    for backend in [Backend::Rust, Backend::System] {
        let built: Vec<(usize, Target)> = targets
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, target)| target.backend == backend)
            .collect();
        if built.is_empty() {
            continue;
        }
        let messages = build(root, backend, built.iter().map(|&(_, target)| target))?;
        for (index, target) in built {
            binaries[index] = Some(test_executable(&messages, target).ok_or(
                Error::MissingTestBinary {
                    package: target.package,
                    name: target.test,
                },
            )?);
        }
    }
    Ok(binaries.into_iter().flatten().collect())
}

/// Builds `targets` with `backend` in one `cargo test --no-run`; Cargo's
/// JSON messages.
fn build(
    root: &Path,
    backend: Backend,
    targets: impl Iterator<Item = Target>,
) -> Result<String, Error> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(&cargo);
    command.current_dir(root).arg("test");
    for target in targets {
        command.args(["--package", target.package, "--test", target.test]);
    }
    command.args(backend.features());
    let output = command
        .args([
            "--no-run",
            "--locked",
            "--message-format=json-render-diagnostics",
        ])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|source| Error::Spawn {
            program: "cargo".into(),
            source,
        })?;
    if !output.status.success() {
        return Err(Error::BuildFailed {
            status: output.status,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The executable of `target` in Cargo's JSON messages.
fn test_executable(messages: &str, target: Target) -> Option<PathBuf> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| message["target"]["name"] == target.test)
        .filter(|message| {
            message["package_id"]
                .as_str()
                .is_some_and(|id| package_name(id) == Some(target.package))
        })
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
}

/// The package name in a Cargo package id: `path+file:///…/name#0.1.0`, or
/// `path+file:///…/dir#name@0.1.0` when the directory is named otherwise.
fn package_name(id: &str) -> Option<&str> {
    let (location, fragment) = id.rsplit_once('#')?;
    match fragment.split_once('@') {
        Some((name, _)) => Some(name),
        None => location.rsplit('/').next(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WM: &str = "deskrinch-window-management";
    const SCREEN: &str = "deskrinch-test-screen";

    const MESSAGES: &str = r#"{"reason":"compiler-artifact","package_id":"path+file:///w/crates/deskrinch-window-management#0.1.0","target":{"name":"deskrinch_window_management","kind":["lib"]},"executable":null}
{"reason":"compiler-artifact","package_id":"path+file:///w/crates/deskrinch-window-management#0.1.0","target":{"name":"compositor","kind":["test"]},"executable":"/target/debug/deps/compositor-0123"}
not json
{"reason":"compiler-artifact","package_id":"path+file:///w/crates/deskrinch-window-management#0.1.0","target":{"name":"kwin_not_granted","kind":["test"]},"executable":"/target/debug/deps/kwin_not_granted-4567"}
{"reason":"compiler-artifact","package_id":"path+file:///w/crates/screen#deskrinch-test-screen@0.1.0","target":{"name":"compositor","kind":["test"]},"executable":"/target/debug/deps/compositor-89ab"}
{"reason":"build-finished","success":true}"#;

    fn target(package: &'static str, test: &'static str) -> Target {
        Target {
            package,
            test,
            backend: Backend::Rust,
        }
    }

    #[test]
    fn the_system_backend_is_a_feature_of_the_client_library() {
        assert!(
            Backend::Rust.features().is_empty(),
            "the default needs no feature"
        );
        assert_eq!(
            Backend::System.features(),
            ["--features", "wayland-client/system"]
        );
    }

    #[test]
    fn finds_each_test_executable_by_package() {
        assert_eq!(
            test_executable(MESSAGES, target(WM, "compositor")),
            Some(PathBuf::from("/target/debug/deps/compositor-0123"))
        );
        assert_eq!(
            test_executable(MESSAGES, target(WM, "kwin_not_granted")),
            Some(PathBuf::from("/target/debug/deps/kwin_not_granted-4567"))
        );
        assert_eq!(
            test_executable(MESSAGES, target(SCREEN, "compositor")),
            Some(PathBuf::from("/target/debug/deps/compositor-89ab"))
        );
    }

    #[test]
    fn a_target_without_an_executable_is_not_found() {
        assert_eq!(
            test_executable(MESSAGES, target(WM, "deskrinch_window_management")),
            None
        );
        assert_eq!(test_executable(MESSAGES, target(WM, "absent")), None);
        assert_eq!(
            test_executable(MESSAGES, target(SCREEN, "kwin_not_granted")),
            None
        );
    }

    #[test]
    fn reads_package_ids() {
        assert_eq!(package_name("path+file:///w/crates/a-b#0.1.0"), Some("a-b"));
        assert_eq!(
            package_name("path+file:///w/crates/dir#a-b@0.1.0"),
            Some("a-b")
        );
        assert_eq!(package_name("no fragment"), None);
    }
}
