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
//! It removes a directory only if it is a session's: directly in `/tmp`,
//! named as a session's, and holding the marker [`MARKER`] the session
//! writes before starting it ([`deskrinch_test_session::root::check`]). It
//! checks that when it starts, refusing to run (and the session then
//! refusing to start) for any other directory, and again before it removes
//! one.
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
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use rustix::process::Pid;

use deskrinch_test_session::root::MARKER;

use crate::error::Error;

/// The hidden subcommand that runs a supervisor.
pub(crate) const SUBCOMMAND: &str = "session-supervisor";

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

/// Whether `root` is a session's root ([`deskrinch_test_session::root::check`]).
pub(crate) fn check(root: &Path) -> Result<(), Error> {
    deskrinch_test_session::root::check(root).map_err(|not| Error::NotASession {
        root: not.root,
        why: not.why,
    })
}

/// Whether a supervisor of the session under `root` runs: a process of this
/// user whose command line is `<program> session-supervisor <root>`.
pub(crate) fn supervised(root: &Path) -> bool {
    let wanted = [SUBCOMMAND.as_bytes(), root.as_os_str().as_bytes()];
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().as_bytes().iter().all(u8::is_ascii_digit))
        .filter_map(|entry| fs::read(entry.path().join("cmdline")).ok())
        .any(|cmdline| {
            let args: Vec<&[u8]> = cmdline.split(|&byte| byte == 0).collect();
            args.get(1..3) == Some(&wanted[..])
        })
}

/// A directory made and marked as a session's root, for tests of what
/// checks one; removed when dropped.
#[cfg(test)]
pub(crate) fn fake_root() -> tempfile::TempDir {
    use deskrinch_test_session::root::{PARENT as ROOT_PARENT, PREFIX as ROOT_PREFIX};
    let root = tempfile::Builder::new()
        .prefix(ROOT_PREFIX)
        .tempdir_in(ROOT_PARENT)
        .unwrap();
    fs::write(root.path().join(MARKER), "").unwrap();
    root
}

#[cfg(test)]
mod tests {
    use deskrinch_test_session::root::{PARENT as ROOT_PARENT, PREFIX as ROOT_PREFIX};

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
    fn a_running_supervisor_is_found_by_its_root() {
        let root = session_like();
        let other = session_like();
        assert!(!supervised(root.path()));
        // A stand-in with a supervisor's command line: a script named as the
        // subcommand, run as `sh session-supervisor ROOT`, waiting on its
        // standard input.
        let scripts = tempfile::tempdir().unwrap();
        fs::write(scripts.path().join(SUBCOMMAND), "read -r line\n").unwrap();
        let mut command = Command::new("sh");
        command
            .current_dir(scripts.path())
            .arg(SUBCOMMAND)
            .arg(root.path())
            .stdin(Stdio::piped());
        let mut stand_in = deskrinch_test_lifetime::spawn(command).unwrap();
        assert!(supervised(root.path()));
        assert!(!supervised(other.path()), "another root's");
        drop(stand_in.stdin.take());
        stand_in.wait().unwrap();
        assert!(!supervised(root.path()), "gone");
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
