//! Starting a process detached from the caller, with a hook that runs while
//! the new process waits to exec.
//!
//! The process is double-forked, as GLib and Qt start detached processes:
//! the child std forks calls `setsid` (a new session, so no controlling
//! terminal and none of the caller's job-control signals), forks the real
//! process and exits at once. The grandchild is then reparented to init or
//! the nearest subreaper, which reaps it; the caller never has a zombie to
//! wait for.
//!
//! Before it execs, the grandchild blocks on a gate pipe. The intermediate
//! child reports the grandchild's PID on a second pipe; a helper thread in
//! the caller reads it, runs the hook (moving the PID into a systemd scope)
//! and opens the gate. So the process runs its first instruction already
//! where the hook put it, and nothing it forks can escape the scope.

use std::io::{self, PipeReader, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread;

/// Spawns `command` detached, runs `before_exec` with its PID while it
/// waits to exec, and returns the PID with what `before_exec` returned.
///
/// The returned PID is the process that runs the program; it is not a
/// child of the caller.
pub(super) fn spawn_detached<T: Send>(
    command: &mut Command,
    before_exec: impl FnOnce(u32) -> T + Send,
) -> io::Result<(u32, T)> {
    let (pid_reader, pid_writer) = io::pipe()?;
    let (gate_reader, gate_writer) = io::pipe()?;
    let fds = ChildFds {
        pid_writer: pid_writer.as_raw_fd(),
        gate_reader: gate_reader.as_raw_fd(),
        gate_writer: gate_writer.as_raw_fd(),
    };
    fds.install(command);
    thread::scope(|scope| {
        let helper = scope.spawn(move || {
            let hooked = read_pid(pid_reader).map(|pid| (pid, before_exec(pid)));
            // Opening the gate takes a byte, not end of file: a process
            // another thread is launching at the same moment has a copy of
            // this write end until it execs, and it may be waiting at its
            // own gate. A one-byte write to an empty pipe cannot block; if
            // it fails, dropping the end still opens the gate once those
            // copies are gone.
            (&gate_writer).write_all(&[1]).ok();
            drop(gate_writer);
            hooked
        });
        let spawned = command.spawn();
        // The child has its own copies; with ours closed, the helper sees
        // end of file if the child never reports a PID.
        drop(pid_writer);
        drop(gate_reader);
        let hooked = helper
            .join()
            .map_err(|_| io::Error::other("the launch helper thread panicked"))?;
        let mut intermediate = spawned?;
        intermediate.wait()?;
        hooked.ok_or_else(|| io::Error::other("the launched process did not report its PID"))
    })
}

/// Reads the PID the intermediate child writes, or `None` at end of file.
fn read_pid(mut reader: PipeReader) -> Option<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(u32::from_ne_bytes(bytes))
}

/// The pipe ends the child needs, as raw descriptors so the pre-exec hook
/// touches nothing but integers.
#[derive(Clone, Copy)]
struct ChildFds {
    pid_writer: RawFd,
    gate_reader: RawFd,
    gate_writer: RawFd,
}

impl ChildFds {
    /// Makes `command` run [`ChildFds::in_child`] between fork and exec.
    #[expect(
        unsafe_code,
        reason = "a pre-exec hook is the only way to double-fork with std"
    )]
    fn install(self, command: &mut Command) {
        // SAFETY: the closure runs in the child between fork and exec, where
        // only async-signal-safe operations are allowed. `in_child` makes
        // only async-signal-safe system calls (setsid, fork, read, write,
        // close, _exit), allocates nothing and takes no locks.
        unsafe {
            command.pre_exec(move || self.in_child());
        }
    }

    /// Runs in the child std forked: becomes a session leader, forks the
    /// grandchild, reports its PID and exits; the grandchild waits at the
    /// gate and returns, so std execs the program in it.
    #[expect(unsafe_code, reason = "setsid, fork and _exit have no safe std form")]
    fn in_child(self) -> io::Result<()> {
        // SAFETY: `setsid` is async-signal-safe and has no memory effects.
        if unsafe { libc::setsid() } == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fork` is async-signal-safe. This process is the single
        // thread std's fork left, so the copy is consistent; both sides
        // make only async-signal-safe calls before exec or `_exit`.
        match unsafe { libc::fork() } {
            -1 => Err(io::Error::last_os_error()),
            0 => {
                self.wait_at_gate();
                Ok(())
            }
            pid => {
                self.report(pid);
                // SAFETY: `_exit` is async-signal-safe and runs no
                // destructors or atexit handlers of the caller's copy.
                unsafe { libc::_exit(0) }
            }
        }
    }

    #[expect(
        unsafe_code,
        reason = "std's writers may allocate, which is unsound after fork"
    )]
    fn report(self, pid: libc::pid_t) {
        let bytes = pid.cast_unsigned().to_ne_bytes();
        // SAFETY: writes four bytes from a live stack buffer to a descriptor
        // this process owns. A write of under `PIPE_BUF` bytes to a pipe is
        // atomic, so there is no partial write to retry. If it fails, the
        // caller sees end of file and reports the launch as failed.
        unsafe { libc::write(self.pid_writer, bytes.as_ptr().cast(), bytes.len()) };
    }

    #[expect(
        unsafe_code,
        reason = "raw descriptors are all a process may touch after fork"
    )]
    fn wait_at_gate(self) {
        // The grandchild's copy of the gate's write end would keep the
        // gate from ever reaching end of file, and the PID pipe is not its
        // business.
        // SAFETY: closes a descriptor this process owns and never uses again.
        unsafe { libc::close(self.gate_writer) };
        // SAFETY: as above.
        unsafe { libc::close(self.pid_writer) };
        let mut byte = 0u8;
        loop {
            // SAFETY: reads at most one byte into a live stack variable.
            let read = unsafe { libc::read(self.gate_reader, (&raw mut byte).cast(), 1) };
            let interrupted =
                read == -1 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted;
            if !interrupted {
                break;
            }
        }
        // SAFETY: closes a descriptor this process owns and never uses again.
        unsafe { libc::close(self.gate_reader) };
    }
}
