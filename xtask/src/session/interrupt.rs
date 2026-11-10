//! Ending sessions when xtask itself is told to stop.
//!
//! A session runs in a process group of its own, which is not the
//! terminal's: Ctrl-C (`SIGINT`), and `SIGTERM` or `SIGHUP`, reach xtask and
//! not the session. The handler installed here ends every live session's
//! process group then and records that xtask was interrupted; the code
//! waiting on the session then finds its command gone, ends the rest of the
//! session as usual, and stops.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use rustix::process::{Pid, Signal, kill_process_group};

use crate::error::Error;

/// The process groups of the sessions running now.
static GROUPS: Mutex<Vec<Pid>> = Mutex::new(Vec::new());
/// Whether a signal asked xtask to stop.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// Whether the handler is installed, or why it could not be.
static HANDLER: OnceLock<Result<(), String>> = OnceLock::new();

/// Installs the handler, once.
pub(super) fn install() -> Result<(), Error> {
    HANDLER
        .get_or_init(|| ctrlc::set_handler(on_signal).map_err(|error| error.to_string()))
        .clone()
        .map_err(|reason| Error::SignalHandler { reason })
}

/// Whether a signal asked xtask to stop.
pub(crate) fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Fails when a signal asked xtask to stop: before starting anything new.
pub(crate) fn check() -> Result<(), Error> {
    if interrupted() {
        Err(Error::Interrupted)
    } else {
        Ok(())
    }
}

/// From now on a signal ends `group` too.
pub(super) fn watch(group: Pid) {
    groups().push(group);
    // A signal that came before the group was known has not ended it.
    if interrupted() {
        end(group);
    }
}

/// `group` has been ended.
pub(super) fn forget(group: Pid) {
    groups().retain(|&watched| watched != group);
}

fn on_signal() {
    INTERRUPTED.store(true, Ordering::SeqCst);
    for &group in groups().iter() {
        end(group);
    }
}

fn end(group: Pid) {
    // The group may have ended already; either way it is asked to.
    let _ = kill_process_group(group, Signal::TERM);
}

fn groups() -> std::sync::MutexGuard<'static, Vec<Pid>> {
    // Nothing panics while holding it, so a poisoned list is still whole.
    GROUPS.lock().unwrap_or_else(PoisonError::into_inner)
}
