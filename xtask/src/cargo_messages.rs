//! Where Cargo put what it built, from its JSON messages
//! (`--message-format=json-render-diagnostics`).

use std::path::PathBuf;

/// The kind of target an executable is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A `[[test]]` target.
    Test,
    /// A `[[bin]]` target.
    Bin,
}

impl Kind {
    /// Cargo's name for it in a target's `kind`.
    fn name(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Bin => "bin",
        }
    }
}

/// The executable of `package`'s `target` of `kind` in Cargo's JSON
/// `messages`.
pub(crate) fn executable(
    messages: &str,
    package: &str,
    target: &str,
    kind: Kind,
) -> Option<PathBuf> {
    messages
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact")
        .filter(|message| message["target"]["name"] == target)
        .filter(|message| {
            message["package_id"]
                .as_str()
                .is_some_and(|id| package_name(id) == Some(package))
        })
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|name| name == kind.name()))
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

    const MESSAGES: &str = r#"{"reason":"compiler-artifact","package_id":"path+file:///w/crates/deskrinch-launcher#0.1.0","target":{"name":"deskrinch_launcher","kind":["lib"]},"executable":null}
{"reason":"compiler-artifact","package_id":"path+file:///w/crates/deskrinch-launcher#0.1.0","target":{"name":"deskrinch-launcher","kind":["bin"]},"executable":"/target/release/deskrinch-launcher"}
{"reason":"compiler-artifact","package_id":"path+file:///w/crates/deskrinch-launcher#0.1.0","target":{"name":"compositor","kind":["test"]},"executable":"/target/debug/deps/compositor-0123"}"#;

    #[test]
    fn finds_an_executable_by_its_kind() {
        let launcher = "deskrinch-launcher";
        assert_eq!(
            executable(MESSAGES, launcher, launcher, Kind::Bin),
            Some(PathBuf::from("/target/release/deskrinch-launcher"))
        );
        assert_eq!(executable(MESSAGES, launcher, launcher, Kind::Test), None);
        assert_eq!(
            executable(MESSAGES, launcher, "compositor", Kind::Test),
            Some(PathBuf::from("/target/debug/deps/compositor-0123"))
        );
        assert_eq!(
            executable(MESSAGES, "deskrinch-panel", launcher, Kind::Bin),
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
