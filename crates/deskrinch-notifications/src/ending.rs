//! How the daemon ends besides its display going away, and the status it
//! ends with.

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use rinch::prelude::exit;

/// Set once the service has ended (its connection, or its name, is gone).
static SERVICE_ENDED: AtomicBool = AtomicBool::new(false);

/// The service ended: the app ends, and the process with a failure, so
/// that the restart policy starts it again.
pub(crate) fn service_ended() {
    SERVICE_ENDED.store(true, Ordering::Relaxed);
    exit();
}

/// The status the process ends with, given the app's: a failure when the
/// service ended, the app's otherwise.
pub(crate) fn status(app: ExitCode) -> ExitCode {
    if SERVICE_ENDED.load(Ordering::Relaxed) {
        ExitCode::FAILURE
    } else {
        app
    }
}
