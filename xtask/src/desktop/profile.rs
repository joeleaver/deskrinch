//! A profile: the home, configuration, data, cache and state a desktop
//! keeps from one run to the next, under `.work/desktop/NAME/` in the
//! workspace (gitignored). One desktop uses a profile at a time.
//!
//! Its home is the session's `HOME`, which also catches what ignores the
//! XDG variables: KWin, run in secure mode for its file capability, has
//! Mesa put its shader cache in `home/.cache`, not in the profile's
//! `cache` (see `deskrinch_test_dbus::session`).
//!
//! Two things keep it to one:
//!
//! - a lock (`flock` on `.lock`) held for as long as `cargo xtask desktop`
//!   runs, which the kernel releases however it ends. The lock belongs to
//!   the lock file's open file description, which a child shares from its
//!   fork until its exec closes it: for a moment after its holder lets go,
//!   a child just forked by another thread can still hold it. A refusal
//!   then is true at that moment, and passes;
//! - the session that uses it, claimed in `.session` (its root and its
//!   process group) while it runs. Should the command be killed, its
//!   session's supervisor is still ending that session, with the profile's
//!   programs, after the lock is gone. The profile is in use while the
//!   claimed root is there and either its supervisor or its process group
//!   still runs; the supervisor removes the root last.
//!
//! A claim can outlive both: a supervisor that was itself killed leaves the
//! root, and the claim naming it, behind. Once neither the supervisor nor
//! any process of the session's group runs, nothing writes to the profile
//! any more, and the next desktop on it clears the claim, saying so, and
//! leaves the root for the owner to remove.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, flock};
use rustix::process::{Pid, test_kill_process_group};

use super::DesktopError;
use crate::error::Error;
use crate::session;

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
        if let Ok(text) = fs::read_to_string(dir.join(SESSION)) {
            let claim = Claim::read(&text);
            if claim.running() {
                return Err(DesktopError::ProfileInUse {
                    profile: dir,
                    why: "the session of a desktop that was killed is still being ended",
                }
                .into());
            }
            if claim.root.exists() {
                eprintln!(
                    "desktop: the profile's claim by {} is stale (neither its supervisor nor any \
                     of its processes runs), and is cleared; that directory is left for you to \
                     remove",
                    claim.root.display()
                );
            }
            let path = dir.join(SESSION);
            fs::remove_file(&path)
                .map_err(|source| io_error(&format!("remove {}", path.display()), source))?;
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

    /// Claims the profile for the session under `session_root`, whose
    /// process group is `group`.
    pub(super) fn claim(&mut self, session_root: &Path, group: Pid) -> Result<(), Error> {
        let path = self.dir.join(SESSION);
        let claim = Claim {
            root: session_root.to_owned(),
            group: Some(group),
        };
        fs::write(&path, claim.text())
            .map_err(|source| io_error(&format!("write {}", path.display()), source))?;
        self.claimed = true;
        Ok(())
    }
}

/// A session's claim on a profile, as `.session` holds it: the session's
/// root on its first line, its process group on the second.
#[derive(Debug, PartialEq, Eq)]
struct Claim {
    root: PathBuf,
    /// `None` when the line is missing or not a process id.
    group: Option<Pid>,
}

impl Claim {
    fn read(text: &str) -> Self {
        let mut lines = text.lines();
        let root = PathBuf::from(lines.next().unwrap_or_default());
        let group = lines
            .next()
            .and_then(|line| line.parse().ok())
            .and_then(Pid::from_raw);
        Self { root, group }
    }

    fn text(&self) -> String {
        let group = self.group.map_or(0, |group| group.as_raw_nonzero().get());
        format!("{}\n{group}\n", self.root.display())
    }

    /// Whether the claiming session may still run: its root is there, and
    /// its supervisor or a process of its group runs.
    fn running(&self) -> bool {
        self.root.exists()
            && (session::supervised(&self.root)
                || self
                    .group
                    .is_some_and(|group| test_kill_process_group(group).is_ok()))
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
    use std::os::fd::{AsRawFd, BorrowedFd};
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    use deskrinch_test_wait::wait_for;
    use rustix::pipe::{PipeFlags, pipe_with};

    use super::*;

    #[test]
    fn names_are_plain() {
        assert!(valid_name("work"));
        assert!(valid_name("test_2-a"));
        for name in ["", "..", "a/b", "a b", ".hidden", &"x".repeat(65)] {
            assert!(!valid_name(name), "{name:?}");
        }
    }

    /// Opens the profile `work` in `workspace` again once its lock is free.
    ///
    /// A lock this test has dropped can still be held, for a moment, by a
    /// child another test's thread has just forked (see the module's doc,
    /// and [`a_child_between_fork_and_exec_holds_the_lock`]). So a profile
    /// reopened in a test binary that spawns on other threads is waited for,
    /// within the patience; any other refusal fails at once.
    #[track_caller]
    fn reopened(workspace: &Path) -> Profile {
        wait_for("the profile's lock to be free", || {
            match Profile::open(workspace, "work") {
                Ok(profile) => Ok(profile),
                Err(Error::Desktop(DesktopError::ProfileInUse {
                    why: "another desktop runs with it",
                    ..
                })) => Err("its lock is held"),
                Err(other) => panic!("the profile refused otherwise: {other}"),
            }
        })
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
        reopened(workspace.path());
    }

    /// The lock's open file description is shared with a child from its
    /// fork until its exec (see the module's doc): a child held between the
    /// two keeps the lock after the profile is dropped, and frees it at its
    /// exec.
    #[test]
    #[expect(
        unsafe_code,
        reason = "pre_exec, to hold a child between its fork and its exec"
    )]
    fn a_child_between_fork_and_exec_holds_the_lock() {
        let workspace = tempfile::tempdir().unwrap();
        let profile = Profile::open(workspace.path(), "work").unwrap();
        // The child says it has forked on one pipe, then waits on another
        // for leave to exec.
        let (forked_read, forked_write) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        let (go_read, go_write) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        let (forked_fd, go_fd, go_write_fd) = (
            forked_write.as_raw_fd(),
            go_read.as_raw_fd(),
            go_write.as_raw_fd(),
        );
        let mut command = Command::new("true");
        let in_child = move || {
            // SAFETY: the child's copy of `go_write`, open until this closes
            // it, and used by nothing else in the child. Closed so that the
            // child reads the end of the pipe, and goes on to its exec,
            // should this test drop its own copy without writing.
            unsafe { rustix::io::close(go_write_fd) };
            // SAFETY: open in the child, inherited from the pipe this test
            // holds, until its exec closes it.
            let forked = unsafe { BorrowedFd::borrow_raw(forked_fd) };
            // SAFETY: as `forked`.
            let go = unsafe { BorrowedFd::borrow_raw(go_fd) };
            let _said = rustix::io::write(forked, b"f");
            let _go = rustix::io::read(go, &mut [0_u8; 1]);
            Ok(())
        };
        // SAFETY: the closure runs in the child between fork and exec,
        // where only async-signal-safe calls may be made. It makes three
        // system calls, `close(2)`, `write(2)` and `read(2)`, on descriptors
        // the child inherited open, allocates nothing, takes no lock, and
        // returns.
        unsafe {
            command.pre_exec(in_child);
        }
        // `spawn` returns only once the child has exec'd. This test's copy
        // of `forked_write` goes with it, so that a spawn that never forks
        // ends the read below instead of leaving it waiting.
        let spawned = std::thread::spawn(move || {
            let child = command.spawn();
            drop(forked_write);
            child.unwrap()
        });
        assert_eq!(
            rustix::io::read(&forked_read, &mut [0_u8; 1]).unwrap(),
            1,
            "the child never forked"
        );
        drop(profile);
        // The child, not yet at its exec, still holds the lock.
        assert!(matches!(
            Profile::open(workspace.path(), "work"),
            Err(Error::Desktop(DesktopError::ProfileInUse {
                why: "another desktop runs with it",
                ..
            }))
        ));
        rustix::io::write(&go_write, b"g").unwrap();
        let mut child = spawned.join().unwrap();
        // Exec'd, so its copy is closed: the profile is free, once no
        // other test's child holds it either.
        reopened(workspace.path());
        child.wait().unwrap();
    }

    /// A process group of its own that runs until killed.
    fn running_group() -> (std::process::Child, Pid) {
        let mut command = Command::new("sleep");
        command.arg("600").process_group(0);
        // Bound to this test process, should it die before killing it.
        let child = deskrinch_test_lifetime::spawn(command).unwrap();
        let group = Pid::from_child(&child);
        (child, group)
    }

    /// Writes the claim of a desktop that was killed: its lock is gone, its
    /// claim stays.
    fn claimed_by(workspace: &Path, root: &Path, group: Pid) -> PathBuf {
        let dir = workspace.join(".work/desktop/work");
        fs::create_dir_all(&dir).unwrap();
        let claim = Claim {
            root: root.to_owned(),
            group: Some(group),
        };
        fs::write(dir.join(SESSION), claim.text()).unwrap();
        dir
    }

    #[test]
    fn a_claim_reads_back() {
        let claim = Claim {
            root: PathBuf::from("/tmp/deskrinch-session-x"),
            group: Pid::from_raw(4242),
        };
        assert_eq!(Claim::read(&claim.text()), claim);
        assert_eq!(
            Claim::read("/tmp/x\n"),
            Claim {
                root: PathBuf::from("/tmp/x"),
                group: None
            }
        );
    }

    #[test]
    fn a_session_still_ending_keeps_it_in_use() {
        let workspace = tempfile::tempdir().unwrap();
        let session = tempfile::tempdir().unwrap();
        let (mut child, group) = running_group();
        let dir = claimed_by(workspace.path(), session.path(), group);
        assert!(matches!(
            Profile::open(workspace.path(), "work"),
            Err(Error::Desktop(DesktopError::ProfileInUse { .. }))
        ));
        // Once the supervisor has ended the group and removed the root, it
        // is free, and a clean end leaves no claim.
        child.kill().unwrap();
        child.wait().unwrap();
        drop(session);
        let mut second = reopened(workspace.path());
        second.claim(Path::new("/nonexistent"), group).unwrap();
        drop(second);
        assert!(!dir.join(SESSION).exists());
    }

    #[test]
    fn a_claim_whose_supervisor_and_processes_are_gone_is_cleared() {
        let workspace = tempfile::tempdir().unwrap();
        // The supervisor was killed too: the root stays, nothing runs.
        let session = tempfile::tempdir().unwrap();
        let (mut child, group) = running_group();
        child.kill().unwrap();
        child.wait().unwrap();
        let dir = claimed_by(workspace.path(), session.path(), group);
        let profile = Profile::open(workspace.path(), "work").unwrap();
        assert!(!dir.join(SESSION).exists(), "cleared");
        assert!(session.path().exists(), "the root is left to its owner");
        drop(profile);
    }
}
