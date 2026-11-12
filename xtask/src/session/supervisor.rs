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
//! The session's bus needs no supervision: its daemon is xtask's child,
//! killed by the kernel when xtask dies (`deskrinch-test-lifetime`).
//!
//! What it cannot do: xtask and its supervisor killed at the same instant
//! (`SIGKILL` to both) still leaves the session running.

use std::io::{self, BufRead as _, BufReader, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use rustix::process::Pid;

use crate::error::Error;

/// The hidden subcommand that runs a supervisor.
pub(crate) const SUBCOMMAND: &str = "session-supervisor";

/// The session's side of the pipe.
#[derive(Debug)]
pub(super) struct Supervisor {
    child: Child,
    pipe: Option<ChildStdin>,
}

impl Supervisor {
    /// Starts the supervisor of the session under `root`.
    pub(super) fn start(root: &Path) -> Result<Self, Error> {
        let spawn = |source| Error::Spawn {
            program: format!("xtask {SUBCOMMAND}"),
            source,
        };
        let mut child = Command::new(std::env::current_exe().map_err(spawn)?)
            .arg(SUBCOMMAND)
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // A process group of its own: outside the session's, and outside
            // the terminal's foreground group.
            .process_group(0)
            .spawn()
            .map_err(spawn)?;
        let pipe = child.stdin.take();
        Ok(Self { child, pipe })
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

/// The supervisor itself (`xtask session-supervisor <root>`): waits for its
/// session to end, and ends it if xtask did not.
pub(crate) fn run(root: &Path) {
    let mut group = None;
    let mut ended = false;
    for line in BufReader::new(io::stdin().lock()).lines() {
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
    if !ended {
        end(group, root);
    }
}

/// Ends the session under `root` whose xtask has gone: its process group,
/// if it was told one, every process that left it, and its directory.
fn end(group: Option<Pid>, root: &Path) {
    if let Some(group) = group {
        // Not the group's parent: the leader's zombie is reaped by whoever
        // inherited it.
        super::end_group(group, || {});
    }
    super::kill_leftovers(&root.join("run"));
    let root: PathBuf = root.to_owned();
    if let Err(error) = std::fs::remove_dir_all(&root) {
        eprintln!(
            "session supervisor: could not remove {}: {error}",
            root.display()
        );
    }
}
