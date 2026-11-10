//! Building the compositor test binaries and finding where Cargo put them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::Error;

/// The crate whose tests run in the compositors.
const PACKAGE: &str = "deskrinch-window-management";

/// The test binaries, by their `[[test]]` names.
#[derive(Debug)]
pub(super) struct TestBinaries {
    /// `compositor`: the crate against a compositor that grants it access.
    pub(super) compositor: PathBuf,
    /// `kwin_not_granted`: KWin's refusal of an ungranted client.
    pub(super) kwin_not_granted: PathBuf,
}

/// Builds the package's integration tests and returns their binaries.
pub(super) fn test_binaries(root: &Path) -> Result<TestBinaries, Error> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let args = [
        "test",
        "--package",
        PACKAGE,
        "--test",
        "compositor",
        "--test",
        "kwin_not_granted",
        "--no-run",
        "--locked",
        "--message-format=json-render-diagnostics",
    ];
    let output = Command::new(&cargo)
        .current_dir(root)
        .args(args)
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
    let messages = String::from_utf8_lossy(&output.stdout);
    let find = |name: &'static str| {
        test_executable(&messages, name).ok_or(Error::MissingTestBinary { name })
    };
    Ok(TestBinaries {
        compositor: find("compositor")?,
        kwin_not_granted: find("kwin_not_granted")?,
    })
}

/// The executable of the test target `name` in Cargo's JSON messages.
fn test_executable(messages: &str, name: &str) -> Option<PathBuf> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| message["target"]["name"] == name)
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
        })
        .find_map(|message| message["executable"].as_str().map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGES: &str = r#"{"reason":"compiler-artifact","target":{"name":"deskrinch_window_management","kind":["lib"]},"executable":null}
{"reason":"compiler-artifact","target":{"name":"compositor","kind":["test"]},"executable":"/target/debug/deps/compositor-0123"}
not json
{"reason":"compiler-artifact","target":{"name":"kwin_not_granted","kind":["test"]},"executable":"/target/debug/deps/kwin_not_granted-4567"}
{"reason":"build-finished","success":true}"#;

    #[test]
    fn finds_each_test_executable() {
        assert_eq!(
            test_executable(MESSAGES, "compositor"),
            Some(PathBuf::from("/target/debug/deps/compositor-0123"))
        );
        assert_eq!(
            test_executable(MESSAGES, "kwin_not_granted"),
            Some(PathBuf::from("/target/debug/deps/kwin_not_granted-4567"))
        );
    }

    #[test]
    fn a_target_without_an_executable_is_not_found() {
        assert_eq!(
            test_executable(MESSAGES, "deskrinch_window_management"),
            None
        );
        assert_eq!(test_executable(MESSAGES, "absent"), None);
    }
}
