//! A session's root directory: where it is made, what it is named, the
//! marker the session writes in it, and the check that a directory is one.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Where a session's root is made: `/tmp`, not `$TMPDIR`, as the Wayland
/// socket's path must fit a Unix socket address (108 bytes), and `$TMPDIR`
/// may be long.
pub const PARENT: &str = "/tmp";

/// What a session's root is named, before its random part.
pub const PREFIX: &str = "deskrinch-session-";

/// The file that marks a directory as a session's root.
pub const MARKER: &str = ".deskrinch-session";

/// Why a directory is not a session's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotARoot {
    /// The directory.
    pub root: PathBuf,
    /// What it lacks.
    pub why: &'static str,
}

impl fmt::Display for NotARoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not a session's root: {}",
            self.root.display(),
            self.why
        )
    }
}

impl std::error::Error for NotARoot {}

/// Whether `root` is a session's root: a directory (not a link to one)
/// directly in [`PARENT`], named with [`PREFIX`], holding [`MARKER`].
///
/// # Errors
///
/// [`NotARoot`], saying what it lacks.
pub fn check(root: &Path) -> Result<(), NotARoot> {
    let refuse = |why| NotARoot {
        root: root.to_owned(),
        why,
    };
    let is_dir = fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_dir());
    if !is_dir {
        return Err(refuse("not a directory"));
    }
    let resolved = fs::canonicalize(root).map_err(|_| refuse("not a directory"))?;
    if resolved.parent() != Some(Path::new(PARENT)) {
        return Err(refuse("not directly in the temporary directory"));
    }
    let named = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(PREFIX));
    if !named {
        return Err(refuse("not named as a session's"));
    }
    if !resolved.join(MARKER).is_file() {
        return Err(refuse("no session marker in it"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A directory made as a session's root, marked unless `marked` is
    /// false; removed when dropped.
    pub(crate) fn made(marked: bool) -> tempfile::TempDir {
        let root = tempfile::Builder::new()
            .prefix(PREFIX)
            .tempdir_in(PARENT)
            .unwrap();
        if marked {
            fs::write(root.path().join(MARKER), "").unwrap();
        }
        root
    }

    #[test]
    fn a_marked_root_is_a_session_s() {
        check(made(true).path()).unwrap();
    }

    #[test]
    fn anything_else_is_refused_with_what_it_lacks() {
        let why = |path: &Path| check(path).unwrap_err().why;
        assert_eq!(why(made(false).path()), "no session marker in it");
        let plain = tempfile::tempdir_in(PARENT).unwrap();
        fs::write(plain.path().join(MARKER), "").unwrap();
        assert_eq!(why(plain.path()), "not named as a session's");
        let deeper = made(true);
        let inner = deeper.path().join(format!("{PREFIX}inner"));
        fs::create_dir(&inner).unwrap();
        fs::write(inner.join(MARKER), "").unwrap();
        assert_eq!(why(&inner), "not directly in the temporary directory");
        assert_eq!(why(&deeper.path().join("missing")), "not a directory");
    }
}
