//! A profile: the home, configuration, data, cache and state a desktop
//! keeps from one run to the next, under `.work/desktop/NAME/` in the
//! workspace (gitignored). One desktop uses a profile at a time.
//!
//! Two things keep it to one:
//!
//! - a lock (`flock` on `.lock`) held for as long as `cargo xtask desktop`
//!   runs, which the kernel releases however it ends;
//! - the session that uses it, named in `.session` while it runs. Should
//!   the command be killed, its session's supervisor is still ending that
//!   session, with the profile's programs, after the lock is gone; the
//!   profile is in use until the session's root is removed, which the
//!   supervisor does last.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};

use super::DesktopError;
use crate::error::Error;

/// Where profiles are kept, under the workspace.
const PROFILES: &str = ".work/desktop";
/// The file locked while a desktop uses the profile.
const LOCK: &str = ".lock";
/// The file naming the session that uses the profile.
const SESSION: &str = ".session";

/// A profile a desktop uses, locked while this lives.
#[derive(Debug)]
pub(super) struct Profile {
    dir: PathBuf,
    /// Holds the lock.
    _lock: File,
    /// Whether this run has named its session in [`SESSION`].
    claimed: bool,
}

impl Profile {
    /// Opens the profile `name` in the workspace at `root`, making it if
    /// new, and locks it.
    ///
    /// # Errors
    ///
    /// [`DesktopError::ProfileName`] for a name other than letters, digits,
    /// `-` and `_`; [`DesktopError::ProfileInUse`] while another desktop
    /// uses it, or its session is still being ended.
    pub(super) fn open(root: &Path, name: &str) -> Result<Self, Error> {
        if !valid_name(name) {
            return Err(DesktopError::ProfileName(name.to_owned()).into());
        }
        let dir = root.join(PROFILES).join(name);
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|source| io_error(&format!("create {}", dir.display()), source))?;
        let lock_path = dir.join(LOCK);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|source| io_error(&format!("open {}", lock_path.display()), source))?;
        if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
            return Err(DesktopError::ProfileInUse {
                profile: dir,
                why: "another desktop runs with it",
            }
            .into());
        }
        if let Ok(session) = fs::read_to_string(dir.join(SESSION))
            && Path::new(session.trim_end()).exists()
        {
            return Err(DesktopError::ProfileInUse {
                profile: dir,
                why: "the session of a desktop that was killed is still being ended",
            }
            .into());
        }
        Ok(Self {
            dir,
            _lock: lock,
            claimed: false,
        })
    }

    /// The profile's directory.
    pub(super) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Names `session_root` as the session that uses the profile.
    pub(super) fn claim(&mut self, session_root: &Path) -> Result<(), Error> {
        let path = self.dir.join(SESSION);
        fs::write(&path, format!("{}\n", session_root.display()))
            .map_err(|source| io_error(&format!("write {}", path.display()), source))?;
        self.claimed = true;
        Ok(())
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        // The session has ended by now (the desktop ends it first); a
        // `.session` left behind names a root that is gone, which the next
        // run ignores.
        if self.claimed {
            let _removed = fs::remove_file(self.dir.join(SESSION));
        }
    }
}

/// Whether `name` is a profile's name: 1 to 64 ASCII letters, digits, `-`
/// and `_`.
fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn io_error(what: &str, source: io::Error) -> Error {
    Error::Io {
        what: what.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plain() {
        assert!(valid_name("work"));
        assert!(valid_name("test_2-a"));
        for name in ["", "..", "a/b", "a b", ".hidden", &"x".repeat(65)] {
            assert!(!valid_name(name), "{name:?}");
        }
    }

    #[test]
    fn one_desktop_at_a_time() {
        let workspace = tempfile::tempdir().unwrap();
        let first = Profile::open(workspace.path(), "work").unwrap();
        assert_eq!(first.dir(), workspace.path().join(".work/desktop/work"));
        assert!(matches!(
            Profile::open(workspace.path(), "work"),
            Err(Error::Desktop(DesktopError::ProfileInUse { .. }))
        ));
        drop(first);
        Profile::open(workspace.path(), "work").unwrap();
    }

    #[test]
    fn a_session_still_ending_keeps_it_in_use() {
        let workspace = tempfile::tempdir().unwrap();
        let session = tempfile::tempdir().unwrap();
        // A desktop that was killed: its lock is gone, its claim stays.
        let dir = workspace.path().join(".work/desktop/work");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SESSION), format!("{}\n", session.path().display())).unwrap();
        assert!(matches!(
            Profile::open(workspace.path(), "work"),
            Err(Error::Desktop(DesktopError::ProfileInUse { .. }))
        ));
        // Once the supervisor has removed the session's root, it is free.
        drop(session);
        let mut second = Profile::open(workspace.path(), "work").unwrap();
        second.claim(Path::new("/nonexistent")).unwrap();
        drop(second);
        assert!(!dir.join(SESSION).exists());
    }
}
