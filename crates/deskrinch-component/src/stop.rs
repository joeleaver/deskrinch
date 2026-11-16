//! Ending in order when the process is asked to stop: `SIGTERM` (how a
//! session or a unit stops a component), `SIGINT` and `SIGHUP`.
//!
//! [`install_once`] registers, once per process, for each of the three
//! that was not ignored when it is called:
//!
//! - **The first** of them is handed to a thread of its own,
//!   `component-stop`, which says so at info level ([`asked_line`]) and
//!   calls the ask it was given: [`crate::app::run`] gives rinch's
//!   `exit`, which ends the event loop on rinch's thread, windows or none,
//!   so that `run` returns success and the component's `main` runs to its
//!   end, its values dropped in order.
//! - **A second**, while the first is acted on, is the signal's default
//!   action at once: the process ends by that signal. It is done in the
//!   signal handler itself (signal-hook's `register_conditional_default`),
//!   so it needs no thread of ours and works however stuck the process
//!   is: a session that sends `SIGTERM` again, or a user pressing Ctrl-C
//!   twice, is not kept waiting. (`SIGKILL` is never caught.)
//! - **A stuck loop:** if the process is still running [`BOUND`] after the
//!   first signal, the thread ends it by that signal's default action,
//!   saying so first as a warning ([`bound_line`]). Nothing makes a
//!   component unkillable by one `SIGTERM`; deskrinch-shell's restart rules
//!   read an end by any of the three as clean, as they always have.
//! - **A signal ignored when the process started** stays ignored. A
//!   component started under `nohup`, or by a shell that ignores `SIGINT`
//!   for a background job, inherits `SIG_IGN`, and installing a handler
//!   over it would undo what its starter asked for. The disposition is
//!   read when [`install_once`] is called.
//!
//! The asked line is at info level: being asked to stop is the normal end
//! of a component, not worth a reader's attention by default. The bound's
//! line is a warning: a component that had to be ended by the bound has a
//! fault (its loop was stuck, or its end took longer than it may), and a
//! reader of its log sees it with the default filter (`warn`).
//!
//! The handlers are process-wide and are signal-hook's, which chains them
//! (`signal-hook-registry`): a program that wants a handler of its own for
//! these signals registers it through signal-hook too, and both run. One
//! that must replace this behaviour, rather than add to it, cannot run its
//! app through [`crate::app::run`].

use std::io;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::flag;
use signal_hook::iterator::Signals;
use signal_hook::low_level::emulate_default_handler;

/// How long the process may take to end after the first signal before it
/// is ended by that signal's default action: an orderly end is dropping
/// values and, for a component on the bus, a few calls (milliseconds);
/// systemd's own `TimeoutStopSec` (90 s by default) is far longer.
pub const BOUND: Duration = Duration::from_secs(5);

/// The signals that ask a component to stop.
pub const SIGNALS: [i32; 3] = [SIGTERM, SIGINT, SIGHUP];

/// The line said when the process is asked to stop by `signal`
/// (deskrinch-shell's, which a component's starter reads).
#[must_use]
pub fn asked_line(signal: i32) -> String {
    deskrinch_shell::exit_status::asked_to_stop_line(name(signal))
}

/// The line said, as a warning, when the process, asked to stop by
/// `signal`, has not ended within `bound` and is ended by the signal.
#[must_use]
pub fn bound_line(signal: i32, bound: Duration) -> String {
    format!(
        "still running {bound:?} after {}; ending by it",
        name(signal)
    )
}

/// The name of one of [`SIGNALS`], as `kill -l` gives it with `SIG`.
fn name(signal: i32) -> &'static str {
    match signal {
        SIGTERM => "SIGTERM",
        SIGINT => "SIGINT",
        SIGHUP => "SIGHUP",
        _ => "a signal",
    }
}

/// Registers for [`SIGNALS`] (see the module's doc), the first time it is
/// called in the process; later calls do nothing and return the first's
/// result. `ask` is what ends the program in order; `bound` how long it
/// may take ([`BOUND`] for a component).
///
/// # Errors
///
/// When a handler cannot be registered or the signal thread cannot be
/// started; the process then ends by the default actions, as before.
pub fn install_once(ask: impl Fn() + Send + 'static, bound: Duration) -> Result<(), StopError> {
    static INSTALLED: OnceLock<Result<(), StopError>> = OnceLock::new();
    INSTALLED.get_or_init(|| install(ask, bound)).clone()
}

/// Why the stop could not be installed: a handler could not be registered
/// or the signal thread started. Shared (`Arc`), so that the one result
/// [`install_once`] keeps can be handed to each caller.
#[derive(Debug, Clone, thiserror::Error)]
#[error("could not register the handlers or start their thread")]
pub struct StopError(#[source] Arc<io::Error>);

impl From<io::Error> for StopError {
    fn from(error: io::Error) -> Self {
        Self(Arc::new(error))
    }
}

fn install(ask: impl Fn() + Send + 'static, bound: Duration) -> Result<(), StopError> {
    let watched: Vec<i32> = SIGNALS
        .into_iter()
        .filter(|&signal| !ignored(signal))
        .collect();
    if watched.is_empty() {
        return Ok(());
    }
    let asked = Arc::new(AtomicBool::new(false));
    for &signal in &watched {
        // In this order: a signal after the first finds `asked` set and is
        // the default action; the first only sets it.
        flag::register_conditional_default(signal, Arc::clone(&asked))?;
        flag::register(signal, Arc::clone(&asked))?;
    }
    let mut signals = Signals::new(&watched)?;
    std::thread::Builder::new()
        .name("component-stop".into())
        .spawn(move || {
            let Some(signal) = signals.forever().next() else {
                return;
            };
            tracing::info!("{}", asked_line(signal));
            ask();
            std::thread::sleep(bound);
            tracing::warn!("{}", bound_line(signal, bound));
            // Ends the process by the signal's default action.
            drop(emulate_default_handler(signal));
        })?;
    Ok(())
}

/// Whether `signal` is ignored (`SIG_IGN`) now: inherited from whoever
/// started the process, since nothing in a component ignores it itself.
#[expect(unsafe_code, reason = "reading a signal's disposition: sigaction")]
fn ignored(signal: i32) -> bool {
    let mut current = std::mem::MaybeUninit::<libc::sigaction>::zeroed();
    // SAFETY: `sigaction` with a null new action changes nothing and only
    // writes the current disposition into `current`, a properly aligned
    // `sigaction` we own; `signal` is one of `SIGNALS`, all valid.
    let read = unsafe { libc::sigaction(signal, std::ptr::null(), current.as_mut_ptr()) };
    if read != 0 {
        return false;
    }
    // SAFETY: `sigaction` returned 0, so it wrote the whole struct (and the
    // zeroed start made every byte initialised either way).
    let current = unsafe { current.assume_init() };
    current.sa_sigaction == libc::SIG_IGN
}

#[cfg(test)]
mod tests;
