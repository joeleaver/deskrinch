//! How a running desktop ends, and why: its window closed, the command
//! after `--` exited, or xtask was interrupted, whichever came first.
//!
//! The window is followed ([`follow`]) before the desktop says it is ready,
//! so that from then on its closing is noticed. The compositor going away
//! is the same ending whenever it is noticed: while the follower connects or
//! in its first exchange, or while it watches. Either way the compositor's
//! own exit status is waited for and decides.

use std::process::ExitStatus;
use std::time::{Duration, Instant};

use super::window::{Seen, Window};
use super::{DesktopError, POLL};
use crate::Outcome;
use crate::error::Error;
use crate::run_in_session::exit_code;
use crate::session::{ClientId, Session, interrupt};

/// How long a compositor whose connection was lost has to be seen ended.
const COMPOSITOR_GONE: Duration = Duration::from_secs(5);

/// What following the desktop's window came to.
#[derive(Debug)]
pub(super) enum Followed {
    /// The window is followed: the desktop notices it closing.
    Window(Window),
    /// The compositor went away before the window could be followed, and
    /// the desktop ended with this outcome.
    Ended(Outcome),
}

/// Follows the desktop's window, in the compositor of `session`.
///
/// When following fails, the compositor's exit status tells why, not the
/// failure's text: if the compositor has ended, or ends within
/// [`COMPOSITOR_GONE`], it went away (its window closed, or it failed), and
/// that is the ending, decided as [`run_until_ended`] decides a compositor
/// gone while watched. If it runs on, the follower failed for a reason of
/// its own, and that is the error ([`DesktopError::Watch`]). An interrupt
/// that came first is the outcome either way.
pub(super) fn follow(session: &mut Session) -> Result<Followed, Error> {
    let error = match Window::follow(&session.socket_path()) {
        Ok(window) => return Ok(Followed::Window(window)),
        Err(error) => error,
    };
    let compositor = wait_for_compositor(session);
    let ended = unfollowed(error, interrupt::interrupted(), compositor);
    if let Ok(Outcome::Failed) = ended {
        report_failure(session, compositor);
    }
    ended.map(Followed::Ended)
}

/// Runs until the window closes, the command exits or xtask is
/// interrupted; says when a component exits meanwhile. `window` is the
/// desktop's, followed ([`follow`]).
///
/// An interrupt that came first is the outcome, whatever is noticed after
/// it: the handler records it before it ends the session's group, so once
/// the compositor, the command or a component is seen gone, the record is
/// read again, and if it is there, that is why they went.
pub(super) fn run_until_ended(
    session: &mut Session,
    mut window: Window,
    components: &[(&'static str, ClientId)],
    program: Option<ClientId>,
) -> Result<Outcome, Error> {
    let mut reported = vec![false; components.len()];
    loop {
        if let Some(status) = program.and_then(|id| session.client_status(id)) {
            return outcome(Ending::Command(status), interrupt::interrupted(), None);
        }
        for ((package, id), reported) in components.iter().zip(&mut reported) {
            if !*reported && let Some(status) = session.client_status(*id) {
                interrupt::check()?;
                eprintln!("desktop: {package} exited ({status}); the desktop runs on without it");
                *reported = true;
            }
        }
        match window.watch(POLL) {
            Seen::Open => interrupt::check()?,
            seen => {
                let compositor = (seen == Seen::Gone)
                    .then(|| wait_for_compositor(session))
                    .flatten();
                let ended = outcome(Ending::Window(seen), interrupt::interrupted(), compositor);
                if let Ok(Outcome::Failed) = ended {
                    report_failure(session, compositor);
                }
                return ended;
            }
        }
    }
}

/// Says that the compositor failed, `compositor` saying how it ended (`None`
/// if it runs on though its connection is lost), with its log.
fn report_failure(session: &Session, compositor: Option<ExitStatus>) {
    let how = compositor.map_or_else(
        || "lost its connection and runs on".to_owned(),
        |status| format!("exited ({status})"),
    );
    eprintln!("desktop: the compositor {how}; its log:\n{}", session.log());
}

/// How the compositor ended, once it has, waiting up to [`COMPOSITOR_GONE`]
/// for it after its connection was lost; `None` if it runs on.
fn wait_for_compositor(session: &mut Session) -> Option<ExitStatus> {
    let deadline = Instant::now() + COMPOSITOR_GONE;
    loop {
        if let Some(status) = session.compositor_status() {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

/// What ended the desktop, as it was seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// The command after `--` exited.
    Command(ExitStatus),
    /// The window: no output left, or the compositor gone.
    Window(Seen),
}

/// The desktop's outcome for `ending`, seen with xtask `interrupted` or
/// not, and with the compositor ended as `compositor` says (when it is
/// gone): an interrupt is the outcome whatever was seen after it; then the
/// command's status; a window closed is a pass; a compositor gone with a
/// failure, or still running though its connection is lost, is a failure.
fn outcome(
    ending: Ending,
    interrupted: bool,
    compositor: Option<ExitStatus>,
) -> Result<Outcome, Error> {
    if interrupted {
        return Err(Error::Interrupted);
    }
    Ok(match ending {
        Ending::Command(status) => Outcome::Exit(exit_code(status)),
        Ending::Window(Seen::Open | Seen::NoOutputs) => Outcome::Passed,
        Ending::Window(Seen::Gone) => match compositor {
            Some(status) if status.success() => Outcome::Passed,
            _ => Outcome::Failed,
        },
    })
}

/// The desktop's outcome when its window could not be followed (`error`),
/// seen with xtask `interrupted` or not, and with the compositor ended as
/// `compositor` says: an interrupt is the outcome; then a compositor that
/// has ended went away, which ends the desktop as [`outcome`] decides for a
/// compositor gone; one that runs on leaves the follower's failure its own,
/// the error.
fn unfollowed(
    error: DesktopError,
    interrupted: bool,
    compositor: Option<ExitStatus>,
) -> Result<Outcome, Error> {
    match compositor {
        Some(_) => outcome(Ending::Window(Seen::Gone), interrupted, compositor),
        None if interrupted => Err(Error::Interrupted),
        None => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::thread;

    use super::*;

    /// Exit statuses as `wait` gives them.
    fn exited(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
    fn killed(signal: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(signal)
    }

    #[test]
    fn an_interrupt_is_the_outcome_whatever_is_seen_after_it() {
        // The interrupt ends the group, so the watch sees the compositor
        // gone (or killed), or the command killed: still the interrupt.
        let seen = [
            Ending::Window(Seen::Gone),
            Ending::Window(Seen::NoOutputs),
            Ending::Command(killed(15)),
        ];
        for ending in seen {
            for compositor in [None, Some(killed(15)), Some(exited(0))] {
                assert!(
                    matches!(outcome(ending, true, compositor), Err(Error::Interrupted)),
                    "{ending:?} {compositor:?}"
                );
            }
        }
    }

    #[test]
    fn without_an_interrupt_what_was_seen_decides() {
        let decide = |ending, compositor| outcome(ending, false, compositor).unwrap();
        assert_eq!(decide(Ending::Command(exited(3)), None), Outcome::Exit(3));
        assert_eq!(
            decide(Ending::Window(Seen::NoOutputs), None),
            Outcome::Passed
        );
        // KWin ends as its window closes.
        let gone = Ending::Window(Seen::Gone);
        assert_eq!(decide(gone, Some(exited(0))), Outcome::Passed);
        assert_eq!(decide(gone, Some(killed(11))), Outcome::Failed);
        assert_eq!(decide(gone, Some(exited(1))), Outcome::Failed);
        assert_eq!(decide(gone, None), Outcome::Failed, "runs on unreachable");
    }

    /// A follow's failure, as [`Window::follow`] gives it.
    fn watch_error() -> DesktopError {
        DesktopError::Watch {
            socket: "/nowhere/wayland-0".into(),
            source: "the connection to the compositor failed".into(),
        }
    }

    #[test]
    fn a_follow_that_failed_is_decided_by_the_compositor() {
        let decide = |interrupted, compositor| unfollowed(watch_error(), interrupted, compositor);
        // Gone: its window closed (KWin) as it was being followed, or it
        // failed.
        assert_eq!(decide(false, Some(exited(0))).unwrap(), Outcome::Passed);
        assert_eq!(decide(false, Some(exited(1))).unwrap(), Outcome::Failed);
        assert_eq!(decide(false, Some(killed(11))).unwrap(), Outcome::Failed);
        // Running on: the follower's own failure.
        assert!(matches!(
            decide(false, None),
            Err(Error::Desktop(DesktopError::Watch { .. }))
        ));
        // An interrupt first is the outcome, whatever was seen after it.
        for compositor in [None, Some(killed(15)), Some(exited(0))] {
            assert!(
                matches!(decide(true, compositor), Err(Error::Interrupted)),
                "{compositor:?}"
            );
        }
    }

    /// Follows a stand-in compositor at a socket in a new directory that is
    /// gone before the first exchange completes: with `accepted`, it
    /// accepts the connection and closes it; without, it is gone before
    /// accepting, and its socket refuses.
    fn follow_a_gone_compositor(accepted: bool) -> DesktopError {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("wayland-0");
        let listener = UnixListener::bind(&socket).unwrap();
        let stand_in = if accepted {
            Some(thread::spawn(move || drop(listener.accept().unwrap())))
        } else {
            drop(listener);
            None
        };
        let followed = Window::follow(&socket);
        if let Some(stand_in) = stand_in {
            stand_in.join().unwrap();
        }
        let error = followed.unwrap_err();
        assert!(
            matches!(&error, DesktopError::Watch { socket: at, .. } if *at == socket),
            "{error}"
        );
        error
    }

    #[test]
    fn a_compositor_gone_before_the_first_exchange_is_an_ending() {
        for accepted in [true, false] {
            // Its status decides, as when it goes while watched.
            let ended = unfollowed(follow_a_gone_compositor(accepted), false, Some(exited(0)));
            assert_eq!(ended.unwrap(), Outcome::Passed, "accepted: {accepted}");
            let ended = unfollowed(follow_a_gone_compositor(accepted), false, Some(killed(6)));
            assert_eq!(ended.unwrap(), Outcome::Failed, "accepted: {accepted}");
        }
    }
}
