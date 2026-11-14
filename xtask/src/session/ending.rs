//! Ending a session's processes: its process group, and those that left it.
//!
//! `SIGKILL` cannot be refused, but it is not instant: the kernel kills a
//! process when it is next scheduled, and a dead process stays in its group
//! as a zombie until its parent reaps it. On a loaded machine both take a
//! while. So after `SIGKILL` the group is waited for until it is gone,
//! reaping as it goes, and is said to have outlived its end only after
//! [`STUCK`]: a hang detector, which only something truly stuck (a process
//! in an uninterruptible sleep that never returns, or a zombie nobody reaps)
//! reaches.
//!
//! Who reaps: the session reaps the processes it started (its compositor,
//! and its clients), being their parent, through the `reap` it is given.
//! Any other process of the group (a client's own children, orphaned) has
//! been reparented to the user's subreaper (the systemd user manager) or to
//! init, which reaps it at once. The supervisor, which ends a session whose
//! xtask died, is no process's parent: the session's processes were all
//! reparented when xtask died, and the subreaper or init reaps them; the
//! wait covers that.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{
    Pid, Signal, kill_process, kill_process_group, test_kill_process, test_kill_process_group,
};

use super::{Leftover, POLL, leftovers};

/// How long a group has to end after `SIGTERM` before it gets `SIGKILL`.
pub(super) const SHUTDOWN: Duration = Duration::from_secs(5);
/// How long a process killed with `SIGKILL` may take to be gone (dead and
/// reaped) before it is taken to be stuck: far beyond what scheduling and
/// reaping take even at a load in the hundreds.
pub(super) const STUCK: Duration = Duration::from_secs(60);

/// Ends every process of the process group `group`: `SIGTERM`, then
/// `SIGKILL` for what is left after [`SHUTDOWN`], then waits for the group
/// to be gone. `reap` reaps the processes of the group whose parent the
/// caller is, so that their zombies do not keep the group; it is called
/// while waiting. Returns whether any of the group is still there [`STUCK`]
/// after the `SIGKILL`.
pub(super) fn end_group(group: Pid, reap: impl FnMut()) -> bool {
    end_group_within(group, SHUTDOWN, STUCK, reap)
}

/// [`end_group`] with its two bounds given.
fn end_group_within(
    group: Pid,
    shutdown: Duration,
    stuck: Duration,
    mut reap: impl FnMut(),
) -> bool {
    // The group may already be gone; either way it is asked to end.
    let _ = kill_process_group(group, Signal::TERM);
    if gone_within(shutdown, &mut reap, || {
        test_kill_process_group(group).is_err()
    }) {
        return false;
    }
    let _ = kill_process_group(group, Signal::KILL);
    !gone_within(stuck, &mut reap, || test_kill_process_group(group).is_err())
}

/// Kills every process that names `runtime` as its `XDG_RUNTIME_DIR` (the
/// session's processes that left its group, [`leftovers`]), and returns
/// them once they are gone, or after [`STUCK`]. They are reported whether
/// they are gone or not: that they were there is the fault.
pub(super) fn kill_leftovers(runtime: &Path) -> Vec<Leftover> {
    let leftovers = leftovers::find(runtime);
    for leftover in &leftovers {
        // Already gone, or not ours to signal: either way not running on.
        let _ = kill_process(leftover.pid, Signal::KILL);
    }
    gone_within(STUCK, &mut || {}, || {
        leftovers
            .iter()
            .all(|leftover| test_kill_process(leftover.pid).is_err())
    });
    leftovers
}

/// Waits up to `bound` until `gone` holds, calling `reap` before each look;
/// whether it came to hold.
fn gone_within(bound: Duration, reap: &mut impl FnMut(), gone: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + bound;
    loop {
        reap();
        if gone() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    use super::*;

    /// A process group of its own whose leader ignores `SIGTERM`, so that
    /// only `SIGKILL` ends it.
    fn stubborn_group() -> (Child, Pid) {
        let mut command = Command::new("sh");
        command
            .args(["-c", "trap '' TERM; echo ready; while :; do sleep 1; done"])
            .process_group(0)
            .stdout(Stdio::piped());
        // Bound to this test process, should it die before ending it.
        let mut child = deskrinch_test_lifetime::spawn(command).unwrap();
        // Once it says so, the trap is set.
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "ready\n");
        let group = Pid::from_child(&child);
        (child, group)
    }

    #[test]
    fn a_leader_reaped_late_after_sigkill_did_not_outlive_its_end() {
        let (mut child, group) = stubborn_group();
        // The leader's zombie is reaped only a second after the `SIGKILL`
        // (sent 200 ms in), as on a loaded machine: until then the group is
        // still there, well past the one look the old verdict took.
        let start = Instant::now();
        let outlived = end_group_within(
            group,
            Duration::from_millis(200),
            Duration::from_secs(30),
            || {
                if start.elapsed() > Duration::from_millis(1200) {
                    drop(child.try_wait());
                }
            },
        );
        assert!(!outlived, "a slow death is not a survivor");
        assert!(start.elapsed() > Duration::from_millis(1200));
        assert!(test_kill_process_group(group).is_err());
    }

    #[test]
    fn a_zombie_nobody_reaps_is_reported_once_stuck() {
        let (mut child, group) = stubborn_group();
        let outlived = end_group_within(
            group,
            Duration::from_millis(200),
            Duration::from_millis(500),
            || {},
        );
        assert!(outlived, "the unreaped leader keeps the group");
        child.wait().unwrap();
    }
}
