//! Waiting for a repeated run within its time limit, and ending it when it
//! overruns.
//!
//! A run that overruns is interrupted first (`SIGINT`): xtask ends its
//! sessions on it, as on Ctrl-C. A run that has not ended [`GRACE`] after
//! the interrupt is killed (`SIGKILL`); what it leaves, its sessions'
//! supervisors end.

use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use crate::error::Error;

/// How long an interrupted run has to end its sessions: a session's group
/// gets `SIGTERM`, then `SIGKILL` 5 s later, and is waited for up to 60 s
/// after that.
pub(super) const GRACE: Duration = Duration::from_secs(120);
/// How often the child is looked at.
const POLL: Duration = Duration::from_millis(50);

/// How a run that overran its limit was ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stopped {
    /// It ended on the interrupt, within the grace.
    Interrupted,
    /// It did not, and was killed.
    Killed,
}

/// How a run ended, waited for within its limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Waited {
    pub(super) status: ExitStatus,
    /// How it was ended after overrunning; `None` if it ended by itself.
    pub(super) stopped: Option<Stopped>,
}

/// Waits for `child` for at most `limit`; past it, interrupts it, waits
/// at most `grace` more, then kills it.
pub(super) fn wait(child: &mut Child, limit: Duration, grace: Duration) -> Result<Waited, Error> {
    if let Some(status) = wait_until(child, Instant::now() + limit)? {
        return Ok(Waited {
            status,
            stopped: None,
        });
    }
    signal(child, Signal::INT);
    if let Some(status) = wait_until(child, Instant::now() + grace)? {
        return Ok(Waited {
            status,
            stopped: Some(Stopped::Interrupted),
        });
    }
    signal(child, Signal::KILL);
    let status = child.wait().map_err(waiting)?;
    Ok(Waited {
        status,
        stopped: Some(Stopped::Killed),
    })
}

/// `child`'s status once it has ended, if it does by `deadline`.
fn wait_until(child: &mut Child, deadline: Instant) -> Result<Option<ExitStatus>, Error> {
    loop {
        if let Some(status) = child.try_wait().map_err(waiting)? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

/// Sends `child`, not yet reaped, `signal`. It cannot have gone: an ended
/// child stays a zombie until it is waited for, so its id is still its own.
fn signal(child: &Child, signal: Signal) {
    let pid = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    if let Some(pid) = pid {
        // It may have ended since it was last looked at: then there is
        // nothing to stop, and the wait that follows reaps it.
        let _ = kill_process(pid, signal);
    }
}

fn waiting(source: std::io::Error) -> Error {
    Error::Io {
        what: "wait for a repeated run".into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;

    use super::*;

    const SHORT: Duration = Duration::from_millis(300);

    #[test]
    fn a_run_within_its_limit_is_left_alone() {
        let mut child = Command::new("true").spawn().unwrap();
        let waited = wait(&mut child, Duration::from_secs(60), SHORT).unwrap();
        assert!(waited.status.success());
        assert_eq!(waited.stopped, None);
    }

    #[test]
    fn an_overrun_is_interrupted_first() {
        let mut child = Command::new("sleep").arg("60").spawn().unwrap();
        let started = Instant::now();
        let waited = wait(&mut child, SHORT, Duration::from_secs(30)).unwrap();
        assert_eq!(waited.stopped, Some(Stopped::Interrupted));
        assert_eq!(waited.status.signal(), Some(Signal::INT.as_raw()));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_run_that_ignores_the_interrupt_is_killed_after_the_grace() {
        // A stand-in that ignores SIGINT, as a run stuck past caring would.
        let mut child = Command::new("sh")
            .args(["-c", "trap '' INT; while :; do sleep 0.05; done"])
            .spawn()
            .unwrap();
        // The limit leaves the shell ample time to set its trap.
        let started = Instant::now();
        let waited = wait(&mut child, SHORT, SHORT).unwrap();
        assert_eq!(waited.stopped, Some(Stopped::Killed));
        assert_eq!(waited.status.signal(), Some(Signal::KILL.as_raw()));
        assert!(started.elapsed() >= SHORT * 2);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
