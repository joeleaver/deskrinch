//! The session's supervisor: ends the session should xtask itself die
//! without ending it.
//!
//! A session ends when xtask ends it, or when xtask gets `SIGINT`, `SIGTERM`
//! or `SIGHUP` ([`super::interrupt`]). xtask killed (`SIGKILL`, a crash)
//! ends nothing, and the kernel's parent-death signal is no help: it reaches
//! only direct children, a session is a process group, and the kernel
//! clears it when the child executes a binary with file capabilities, as
//! `kwin_wayland` has (`cap_sys_nice`).
//!
//! So every session has a supervisor: xtask itself, run again with the
//! hidden subcommand [`SUBCOMMAND`], in a process group of its own (outside
//! the session's, and outside xtask's, so that neither a signal to the
//! session nor the terminal's `Ctrl-C` reaches it), started before anything
//! of the session is. It reads a pipe whose other end the session holds:
//! the session's process group once the compositor runs (`group <pid>`),
//! then `ended` when the session ends normally. Should the pipe close
//! without `ended`, xtask has gone, and the supervisor ends the session as
//! the session would have: the group (`SIGTERM`, then `SIGKILL`), every
//! process that left it ([`super::leftovers`]), and the directory.
//!
//! It removes a directory only if it is a session's: directly in
//! [`super::ROOT_PARENT`], named with [`super::ROOT_PREFIX`], and holding
//! the marker [`MARKER`] the session writes before starting it. It checks
//! that when it starts, refusing to run (and the session then refusing to
//! start) for any other directory, and again before it removes one.
//!
//! The session's bus needs no supervision: its daemon is xtask's child,
//! killed by the kernel when xtask dies (`deskrinch-test-lifetime`).
//!
//! What it cannot do:
//!
//! - xtask and its supervisor killed at the same instant (`SIGKILL` to
//!   both) still leaves the session running.
//! - Between the compositor's start and the session telling the supervisor
//!   its process group (microseconds), xtask killed leaves a compositor the
//!   supervisor knows nothing of but its environment. It finds sway and
//!   labwc by that ([`super::leftovers`]), but not KWin: a binary with file
//!   capabilities is not dumpable, so its environment cannot be read even
//!   by the same user.

use std::fs;
use std::io::{self, BufRead, BufReader, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use rustix::process::Pid;

use super::{ROOT_PARENT, ROOT_PREFIX};
use crate::error::Error;

/// The hidden subcommand that runs a supervisor.
pub(crate) const SUBCOMMAND: &str = "session-supervisor";

/// The file that marks a directory as a session's root.
pub(super) const MARKER: &str = ".deskrinch-session";

/// What the supervisor says once it has checked its root.
const READY: &str = "ready";

/// The program this process runs, as the kernel keeps it: whatever has
/// happened to its file since.
const RUNNING_PROGRAM: &str = "/proc/self/exe";

/// The session's side of the pipe.
#[derive(Debug)]
pub(super) struct Supervisor {
    child: Child,
    pipe: Option<ChildStdin>,
}

impl Supervisor {
    /// Marks `root` as a session's, and starts its supervisor; once it has
    /// said it is ready, having checked `root`.
    pub(super) fn start(root: &Path) -> Result<Self, Error> {
        fs::write(root.join(MARKER), "").map_err(|source| Error::Io {
            what: format!("mark {} as a session's", root.display()),
            source,
        })?;
        let spawn = |source| Error::Spawn {
            program: format!("xtask {SUBCOMMAND}"),
            source,
        };
        // This very program, by `/proc/self/exe`, not by its path: Cargo may
        // have rebuilt the file at that path since this started (as
        // `compositor-tests` does, building xtask's own tests), and the path
        // then names another program, or nothing while it is replaced.
        let mut child = Command::new(RUNNING_PROGRAM)
            .arg(SUBCOMMAND)
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // A process group of its own: outside the session's, and outside
            // the terminal's foreground group.
            .process_group(0)
            .spawn()
            .map_err(spawn)?;
        let pipe = child.stdin.take();
        let mut said = String::new();
        if let Some(stdout) = child.stdout.take() {
            // A supervisor that refused its root says nothing, and ends.
            let _read = BufReader::new(stdout).read_line(&mut said);
        }
        let supervisor = Self { child, pipe };
        if said.trim_end() == READY {
            Ok(supervisor)
        } else {
            supervisor.release();
            Err(Error::SupervisorNotReady)
        }
    }

    /// Tells the supervisor the session's process group.
    pub(super) fn group(&mut self, group: Pid) -> Result<(), Error> {
        self.say(&format!("group {}\n", group.as_raw_nonzero()))
            .map_err(|source| Error::Io {
                what: "tell the session's supervisor its process group".into(),
                source,
            })
    }

    /// The session has ended normally: the supervisor leaves, and is waited
    /// for.
    pub(super) fn release(mut self) {
        // Should it have gone already, there is nothing left for it to do.
        let _said = self.say("ended\n");
        drop(self.pipe.take());
        let _waited = self.child.wait();
    }

    fn say(&mut self, line: &str) -> io::Result<()> {
        let pipe = self.pipe.as_mut().ok_or(io::ErrorKind::BrokenPipe)?;
        pipe.write_all(line.as_bytes())?;
        pipe.flush()
    }
}

/// The supervisor itself (`xtask session-supervisor <root>`): checks that
/// `root` is a session's, says it is ready, waits for the session to end,
/// and ends it if xtask did not.
///
/// # Errors
///
/// [`Error::NotASession`] when `root` is not a session's root, at the start
/// or when it would be removed; nothing is done to it then.
pub(crate) fn run(root: &Path) -> Result<(), Error> {
    check(root)?;
    println!("{READY}");
    io::stdout().flush().map_err(|source| Error::Io {
        what: "say the supervisor is ready".into(),
        source,
    })?;
    supervise(root, io::stdin().lock())
}

/// Reads `input` until it ends, and ends the session under `root` unless it
/// said the session had ended.
fn supervise(root: &Path, input: impl BufRead) -> Result<(), Error> {
    let mut group = None;
    let mut ended = false;
    for line in input.lines() {
        let Ok(line) = line else {
            break;
        };
        match line.as_str() {
            "ended" => ended = true,
            line => {
                if let Some(pid) = line.strip_prefix("group ") {
                    group = pid.parse().ok().and_then(Pid::from_raw);
                }
            }
        }
    }
    if ended {
        return Ok(());
    }
    if let Some(group) = group {
        // Not the group's parent: the leader's zombie is reaped by whoever
        // inherited it when xtask died (the user's subreaper, or init), and
        // `end_group` waits for that.
        super::ending::end_group(group, || {});
    }
    super::ending::kill_leftovers(&root.join("run"));
    check(root)?;
    fs::remove_dir_all(root).map_err(|source| Error::Io {
        what: format!("remove {}", root.display()),
        source,
    })
}

/// Whether `root` is a session's root: a directory (not a link to one)
/// directly in [`ROOT_PARENT`], named with [`ROOT_PREFIX`], holding
/// [`MARKER`].
pub(crate) fn check(root: &Path) -> Result<(), Error> {
    let refuse = |why| Error::NotASession {
        root: root.to_owned(),
        why,
    };
    let is_dir = fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_dir());
    if !is_dir {
        return Err(refuse("not a directory"));
    }
    let resolved: PathBuf = fs::canonicalize(root).map_err(|_| refuse("not a directory"))?;
    if resolved.parent() != Some(Path::new(ROOT_PARENT)) {
        return Err(refuse("not directly in the temporary directory"));
    }
    let named = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(ROOT_PREFIX));
    if !named {
        return Err(refuse("not named as a session's"));
    }
    if !resolved.join(MARKER).is_file() {
        return Err(refuse("no session marker in it"));
    }
    Ok(())
}

/// A directory made and marked as a session's root, for tests of what
/// checks one; removed when dropped.
#[cfg(test)]
pub(crate) fn fake_root() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix(ROOT_PREFIX)
        .tempdir_in(ROOT_PARENT)
        .unwrap();
    fs::write(root.path().join(MARKER), "").unwrap();
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory in `ROOT_PARENT` named as a session's, removed when
    /// dropped.
    fn session_like() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(ROOT_PREFIX)
            .tempdir_in(ROOT_PARENT)
            .unwrap()
    }

    #[test]
    fn only_a_marked_session_root_is_a_session_s() {
        let root = session_like();
        assert!(matches!(
            check(root.path()),
            Err(Error::NotASession {
                why: "no session marker in it",
                ..
            })
        ));
        fs::write(root.path().join(MARKER), "").unwrap();
        check(root.path()).unwrap();

        // Marked, but elsewhere: a session's name and marker inside another
        // directory.
        let outer = tempfile::tempdir_in(ROOT_PARENT).unwrap();
        let inner = outer.path().join(format!("{ROOT_PREFIX}x"));
        fs::create_dir(&inner).unwrap();
        fs::write(inner.join(MARKER), "").unwrap();
        assert!(matches!(
            check(&inner),
            Err(Error::NotASession {
                why: "not directly in the temporary directory",
                ..
            })
        ));

        // Marked, in place, but not named as a session's.
        let other = tempfile::tempdir_in(ROOT_PARENT).unwrap();
        fs::write(other.path().join(MARKER), "").unwrap();
        assert!(matches!(
            check(other.path()),
            Err(Error::NotASession {
                why: "not named as a session's",
                ..
            })
        ));
    }

    #[test]
    fn an_unmarked_root_is_not_removed_when_the_pipe_closes() {
        let root = session_like();
        let outcome = supervise(root.path(), io::empty());
        assert!(matches!(outcome, Err(Error::NotASession { .. })));
        assert!(root.path().is_dir());
    }

    #[test]
    fn a_marked_root_is_removed_when_the_pipe_closes_and_kept_when_ended() {
        let root = session_like();
        fs::write(root.path().join(MARKER), "").unwrap();
        supervise(root.path(), "ended\n".as_bytes()).unwrap();
        assert!(root.path().is_dir());
        supervise(root.path(), io::empty()).unwrap();
        assert!(!root.path().exists());
    }
}
