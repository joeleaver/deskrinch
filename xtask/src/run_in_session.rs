//! `cargo xtask session <compositor> <program> [args]`: runs a command as a
//! client of a nested, headless compositor session, for checking a shell
//! component by hand (running it, taking screenshots) without going near
//! the real session.
//!
//! The session is the one `compositor-tests` uses ([`Session`]), so the
//! command and everything it starts get the session's private directories,
//! bus and sockets and none of the real session's, and run in the
//! session's process group: when the command exits, the session and
//! anything the command left running are ended together.

use std::ffi::OsString;

use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, Session};

/// Runs `program` with `args` in a fresh session of `compositor`. Passes
/// when the command exits successfully; on failure the compositor's log is
/// printed.
pub(crate) fn run(
    compositor: Compositor,
    program: &OsString,
    args: &[OsString],
) -> Result<Outcome, Error> {
    let session = Session::start(compositor, |_| Ok(()))?;
    let status = session
        .client(program)
        .args(args)
        .status()
        .map_err(|source| Error::Spawn {
            program: program.to_string_lossy().into_owned(),
            source,
        })?;
    if status.success() {
        Ok(Outcome::Passed)
    } else {
        eprintln!(
            "session: the command failed ({status}); {} log:\n{}",
            compositor.name(),
            session.log()
        );
        Ok(Outcome::Failed)
    }
}
