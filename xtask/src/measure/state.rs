//! `--state DIR`: each run's state directory (`XDG_STATE_HOME`) starts with
//! a copy of `DIR`'s contents, such as the launcher's record of starts
//! (`deskrinch/starts.toml`), so that what is measured is a program with
//! that state. The report says so.

use std::fs;
use std::io;
use std::path::Path;

use crate::error::Error;

/// Checks, before anything starts, that `dir` is a directory that can be
/// read.
pub(super) fn check(dir: &Path) -> Result<(), Error> {
    let unreadable = |source| Error::Io {
        what: format!("read the state to seed, {}", dir.display()),
        source,
    };
    if !fs::metadata(dir).map_err(unreadable)?.is_dir() {
        return Err(unreadable(io::Error::new(
            io::ErrorKind::NotADirectory,
            "--state names a directory",
        )));
    }
    fs::read_dir(dir).map_err(unreadable)?;
    Ok(())
}

/// Copies `dir`'s contents into a session's `state` directory, keeping
/// each file's permissions.
pub(super) fn seed(dir: &Path, state: &Path) -> Result<(), Error> {
    copy_tree(dir, state).map_err(|source| Error::Io {
        what: format!(
            "seed the session's state {} from {}",
            state.display(),
            dir.display()
        ),
        source,
    })
}

/// The report's line naming the seeded state, if any.
pub(super) fn note(dir: Option<&Path>) -> Option<String> {
    dir.map(|dir| format!("state seeded from {}", dir.display()))
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
