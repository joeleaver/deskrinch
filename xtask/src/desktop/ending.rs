//! How a running desktop ends, and why: its window closed, the command
//! after `--` exited, or xtask was interrupted, whichever came first.

use std::process::ExitStatus;
use std::time::{Duration, Instant};

use super::POLL;
use super::window::{Seen, Window};
use crate::Outcome;
use crate::error::Error;
use crate::run_in_session::exit_code;
use crate::session::{ClientId, Session, interrupt};

/// How long a compositor whose connection was lost has to be seen ended.
const COMPOSITOR_GONE: Duration = Duration::from_secs(5);

/// Runs until the window closes, the command exits or xtask is
/// interrupted; says when a component exits meanwhile.
///
/// An interrupt that came first is the outcome, whatever is noticed after
/// it: the handler records it before it ends the session's group, so once
/// the compositor, the command or a component is seen gone, the record is
/// read again, and if it is there, that is why they went.
pub(super) fn run_until_ended(
    session: &mut Session,
    components: &[(&'static str, ClientId)],
    program: Option<ClientId>,
) -> Result<Outcome, Error> {
    let mut window = match Window::follow(&session.socket_path()) {
        Ok(window) => window,
        Err(error) => {
            interrupt::check()?;
            return Err(error.into());
        }
    };
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
                    let how = compositor.map_or_else(
                        || "lost its connection and runs on".to_owned(),
                        |status| format!("exited ({status})"),
                    );
                    eprintln!("desktop: the compositor {how}; its log:\n{}", session.log());
                }
                return ended;
            }
        }
    }
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

#[cfg(test)]
mod tests {
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
}
