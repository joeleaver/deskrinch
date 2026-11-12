//! What a test process starts, ended with it however it ends.
//!
//! A test ends what it started when it drops it: a bus's daemon, a
//! component under test. A test process that is killed (`SIGKILL` from a
//! timeout, a harness that gives up, a watchdog that leaves a thread
//! stalled and exits) runs no destructor, and what it started would run on
//! in the user's session. This crate ties both kinds of thing a test starts
//! to the process itself:
//!
//! - [`spawn`] starts a child that the kernel kills (`SIGKILL`) when this
//!   process ends, however it ends: Linux's parent-death signal, set
//!   between fork and exec.
//! - [`owned_dir`] makes a temporary directory named for this process; the
//!   next call of it, in any test process, removes the directories of
//!   processes that have gone.
//!
//! # The parent-death signal and threads
//!
//! The kernel sends the signal when the *thread* that forked the child
//! ends, not the process: a child forked from a test's thread would be
//! killed when that test ends, though another test still uses it (a shared
//! bus). So every child is forked by one thread of this crate's own, which
//! lives as long as the process.
//!
//! # What is not guaranteed
//!
//! - **The child's own children.** Only the child gets the signal: the
//!   services a bus activates, or the programs a component starts, are not
//!   killed with it. A child that ends them when it ends (a bus's services
//!   lose their bus) takes them with it; nothing makes it.
//! - **A program with file capabilities or set-user-id.** The kernel
//!   clears the parent-death signal when the child executes one
//!   (`kwin_wayland` has `cap_sys_nice`, say), and such a child outlives a
//!   killed test. `cargo xtask session`, which starts KWin, has a
//!   supervisor of its own for that.
//! - **A directory, at once.** No kernel mechanism removes a directory
//!   with its owner; a dead process's directory goes at the next
//!   [`owned_dir`], which may be the next test run.

mod dir;

use std::io;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread;

use rustix::process::{Pid, Signal, getpid, getppid, set_parent_process_death_signal};

pub use dir::owned_dir;

/// Starts `command` as a child that the kernel kills (`SIGKILL`) when this
/// process ends, however it ends; see the crate's documentation for what is
/// not guaranteed.
///
/// The child is forked by one thread of this crate's own that lives as
/// long as the process, so it outlives the thread that asked for it.
///
/// # Errors
///
/// As [`Command::spawn`]; and when this crate's thread cannot be started,
/// or this process ended in the meantime.
pub fn spawn(mut command: Command) -> io::Result<Child> {
    bind(&mut command);
    let spawner = spawner()?;
    let (reply, child) = mpsc::channel();
    spawner
        .send(Request { command, reply })
        .map_err(|_| io::Error::other("the spawning thread has ended"))?;
    child
        .recv()
        .map_err(|_| io::Error::other("the spawning thread has ended"))?
}

/// Whether the process `pid` runs: neither gone nor a zombie (ended, not
/// yet reaped). For tests of what outlives what.
#[must_use]
pub fn running(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // The command name, in parentheses, may hold spaces: the state is the
    // first field after the last `)`.
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next());
    !matches!(state, Some("Z" | "X"))
}

/// A command for the spawning thread, and where its child goes.
struct Request {
    command: Command,
    reply: Sender<io::Result<Child>>,
}

/// The thread that forks every child, started once; `None` when it could
/// not be.
fn spawner() -> io::Result<&'static Sender<Request>> {
    static SPAWNER: OnceLock<Option<Sender<Request>>> = OnceLock::new();
    SPAWNER
        .get_or_init(|| {
            let (requests, incoming) = mpsc::channel::<Request>();
            thread::Builder::new()
                .name("test-lifetime-spawner".into())
                .spawn(move || {
                    // It never ends while the process runs: the sender is
                    // a static, never dropped.
                    for Request { mut command, reply } in incoming {
                        // The asker may have gone; its child is then
                        // dropped, and killed with this process.
                        let _sent = reply.send(command.spawn());
                    }
                })
                .ok()
                .map(|_| requests)
        })
        .as_ref()
        .ok_or_else(|| io::Error::other("the spawning thread could not be started"))
}

/// Makes `command`'s child get `SIGKILL` when its parent ends.
#[expect(
    unsafe_code,
    reason = "a pre-exec hook is the only way to set the child's parent-death signal with std"
)]
fn bind(command: &mut Command) {
    let parent = getpid();
    // SAFETY: the closure runs in the child between fork and exec, where
    // only async-signal-safe operations are allowed. `in_child` makes only
    // two system calls, `prctl` and `getppid`, through rustix's safe
    // wrappers (both async-signal-safe), allocates nothing (its error is a
    // raw OS error) and takes no lock; it captures a `Pid`, which is `Copy`.
    unsafe {
        command.pre_exec(move || in_child(parent));
    }
}

/// In the child: asks for `SIGKILL` when the parent ends, then checks that
/// the parent, `parent`, had not already ended (the child would then be
/// another's, and never get the signal).
fn in_child(parent: Pid) -> io::Result<()> {
    set_parent_process_death_signal(Some(Signal::KILL))?;
    if getppid() == Some(parent) {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            rustix::io::Errno::SRCH.raw_os_error(),
        ))
    }
}
