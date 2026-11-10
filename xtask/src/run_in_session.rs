//! `cargo xtask session`: runs a command as a client of a nested, headless
//! compositor in an isolated [`Session`], then ends the whole session.
//!
//! This is the one way to run something under a nested compositor (a shell
//! component, a rinch example, a screenshot tool) for deskrinch and the
//! projects it works with: the command and everything it starts get the
//! session's private directories, bus and sockets and none of the real
//! session's, and nothing of the session outlives it.

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, Session, interrupt};

/// `cargo xtask session --help`: what the session is, exactly.
pub(crate) const LONG_ABOUT: &str = "\
Run a command in an isolated, nested, headless compositor session.

The command runs as the only client of a fresh nested compositor (KWin, sway
or labwc, headless), and the whole session is ended when it exits. Use this,
never a hand-made session (plain dbus-run-session, a bare kwin_wayland), to run
anything under a nested compositor.

Isolation the command and everything it starts get:
  - XDG_RUNTIME_DIR, XDG_DATA_HOME, XDG_CONFIG_HOME, XDG_CACHE_HOME and
    XDG_STATE_HOME: private directories under a fresh /tmp/deskrinch-session-*
    (also in DESKRINCH_SESSION_DIR). HOME is not changed.
  - None of the real session's DISPLAY, XAUTHORITY, WAYLAND_DISPLAY,
    WAYLAND_SOCKET, SWAYSOCK, I3SOCK or DBUS_SESSION_BUS_ADDRESS.
  - WAYLAND_DISPLAY naming the nested compositor (and SWAYSOCK its IPC under
    sway); no X server (KWin runs without Xwayland, so no kcminit either).
  - DBUS_SESSION_BUS_ADDRESS naming a private dbus-daemon with service
    activation off: nothing (portals, secret service, notification daemons)
    is started on demand. The compositor uses the same bus.
  - Standard input closed. The session's process group is not the
    terminal's, so a command that read the terminal would hang.
  - One process group with the compositor. When the command exits, or when
    this task gets SIGINT, SIGTERM or SIGHUP, the group is sent SIGTERM, then
    SIGKILL after 5 s, and the bus is stopped. Then any process still carrying
    the session's XDG_RUNTIME_DIR (one that left the group) is killed and
    reported as a failure, and the session's directory is kept.

The command's working directory is the caller's. The task exits with the
command's status (128 + N if signal N ended it), 130 if interrupted, and 70 if
the session could not be set up or did not end cleanly.

From outside this repository (a rinch clone under .work/, say), name its
xtask manifest; the command still runs in the current directory:
  cargo run -q --manifest-path /path/to/deskrinch/xtask/Cargo.toml -- \\
      session sway sh -c 'foot & sleep 2; grim shot.png'
Cargo takes its toolchain from the current directory: a rinch checkout pins
deskrinch's; elsewhere add +<channel>, the one deskrinch's
rust-toolchain.toml names.

`--keep` keeps the session's directory, with the compositor's log.";

/// The exit status when a signal ended xtask (as a shell reports SIGINT).
pub(crate) const INTERRUPTED: u8 = 130;
/// The exit status when the session could not be set up, or did not end
/// cleanly (`EX_SOFTWARE`), so that it is not mistaken for the command's.
pub(crate) const SESSION_FAILED: u8 = 70;

/// What `cargo xtask session` was asked to run.
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) compositor: Compositor,
    pub(crate) outputs: u32,
    pub(crate) keep: bool,
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
}

/// Runs the request's command in a fresh session and returns its exit
/// status, once the session has ended.
pub(crate) fn run(request: &Request) -> Result<Outcome, Error> {
    let mut session = Session::start(request.compositor, request.outputs, |_| Ok(()))?;
    if request.keep {
        session.keep();
    }
    eprintln!(
        "session: {} with {} output(s) in {}",
        request.compositor.name(),
        request.outputs,
        session.root().display()
    );
    let status = session
        .client(&request.program)
        .args(&request.args)
        .status();
    if !session.compositor_running() {
        eprintln!(
            "session: {} exited while the command ran; its log:\n{}",
            request.compositor.name(),
            session.log()
        );
    }
    session.end()?;
    interrupt::check()?;
    let status = status.map_err(|source| Error::Spawn {
        program: request.program.to_string_lossy().into_owned(),
        source,
    })?;
    Ok(Outcome::Exit(exit_code(status)))
}

/// The exit status to pass on for a command that ended with `status`: its
/// own code, or 128 plus the signal that ended it, as a shell reports it.
fn exit_code(status: ExitStatus) -> u8 {
    let code = status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(i32::from(SESSION_FAILED));
    u8::try_from(code).unwrap_or(SESSION_FAILED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_the_commands_code_on() {
        assert_eq!(exit_code(ExitStatus::from_raw(0)), 0);
        assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(exit_code(ExitStatus::from_raw(255 << 8)), 255);
    }

    #[test]
    fn reports_a_signal_as_a_shell_does() {
        // Raw wait statuses: the low bits are the signal that ended it.
        assert_eq!(exit_code(ExitStatus::from_raw(15)), 128 + 15);
        assert_eq!(exit_code(ExitStatus::from_raw(9)), 128 + 9);
    }
}
