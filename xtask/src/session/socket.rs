//! The session's first connection to its compositor: the session is ready
//! for clients only once a connection to the compositor's Wayland socket
//! has been accepted, not when the socket file is there.
//!
//! A socket file can be there before anything accepts on it: a compositor
//! binds its socket and listens on it a moment later, and one that has died
//! leaves its socket behind. Both refuse a connection. The session owns the
//! compositor's process, so between two tries it looks at it: a compositor
//! that has exited is reported as that, with its status and its log; one
//! that runs and goes on refusing, at the patience, with the last refusal.

use std::thread;

use deskrinch_test_wait::Deadline;
use deskrinch_test_wayland::{Connecting, connect_when_accepting};

use super::{Compositor, CompositorLog, POLL, Session, compositor, interrupt};
use crate::error::Error;

impl Session {
    /// Waits until the compositor accepts a connection on its Wayland socket
    /// and returns the socket's name: first for the socket to be there,
    /// then for a connection to it to be accepted
    /// ([`connect_when_accepting`]), which a compositor between binding and
    /// listening, or one that has died leaving its socket behind, refuses.
    /// A compositor that exits meanwhile fails at once, with its status and
    /// its log; one that runs and does not accept, at the patience.
    pub(super) fn wait_for_socket(&mut self) -> Result<String, Error> {
        let runtime = self.root.join("run");
        let deadline = Deadline::patience();
        let socket = loop {
            if let Some(socket) = compositor::wayland_socket(&runtime) {
                break socket;
            }
            self.between_tries()?;
            if deadline.passed() {
                return Err(Error::CompositorTimedOut {
                    compositor: self.compositor.name(),
                    why: "no socket".to_owned(),
                    log: self.log(),
                });
            }
        };
        let accepted =
            connect_when_accepting(&runtime.join(&socket), deadline, || self.between_tries());
        match accepted {
            Ok(_stream) => Ok(socket),
            Err(connecting) => Err(not_accepted(self.compositor, connecting, self.log())),
        }
    }

    /// Between two looks at the compositor's socket: fails if xtask was
    /// interrupted or the compositor has exited, and otherwise waits
    /// [`POLL`].
    fn between_tries(&mut self) -> Result<(), Error> {
        interrupt::check()?;
        self.exited()?;
        thread::sleep(POLL);
        Ok(())
    }

    /// `error`, from talking to the compositor after the start, unless the
    /// compositor has exited: then its exit, with its status and its log,
    /// which is what the error came of.
    pub(crate) fn unless_exited(&mut self, error: Error) -> Error {
        self.exited().err().unwrap_or(error)
    }

    /// Fails with [`Error::CompositorExited`], its status and its log, if
    /// the compositor has exited.
    pub(crate) fn exited(&mut self) -> Result<(), Error> {
        match self.child.try_wait() {
            Ok(Some(status)) => Err(Error::CompositorExited {
                compositor: self.compositor.name(),
                status,
                log: self.log(),
            }),
            _ => Ok(()),
        }
    }
}

/// The error for `compositor`'s socket accepting no connection, as
/// [`connect_when_accepting`] says why, with the compositor's `log`: what
/// stopped the tries (it exited, an interrupt) as it is; a socket that
/// refused until the patience as a compositor that runs and does not
/// accept; any other failure as the I/O error it is.
fn not_accepted(
    compositor: Compositor,
    connecting: Connecting<Error>,
    log: CompositorLog,
) -> Error {
    match connecting {
        Connecting::Stopped(error) => error,
        Connecting::NotAccepting { last, .. } => Error::CompositorTimedOut {
            compositor: compositor.name(),
            why: format!("the last try: {last}"),
            log,
        },
        Connecting::Failed { socket, source } => Error::Io {
            what: format!("connect to {}", socket.display()),
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::ExitStatus;

    use super::*;

    #[test]
    fn an_exit_seen_between_tries_is_reported_as_the_exit() {
        let exited = Error::CompositorExited {
            compositor: "sway",
            status: ExitStatus::from_raw(1 << 8),
            log: CompositorLog::default(),
        };
        let error = not_accepted(
            Compositor::Sway,
            Connecting::Stopped(exited),
            CompositorLog::default(),
        );
        let said = error.to_string();
        assert!(
            said.starts_with("sway exited (exit status: 1) before it was ready"),
            "{said}"
        );
        assert!(
            said.ends_with("(empty: the compositor had printed nothing)"),
            "{said}"
        );
    }

    #[test]
    fn a_socket_refusing_until_the_patience_is_a_compositor_that_does_not_accept() {
        let error = not_accepted(
            Compositor::Labwc,
            Connecting::NotAccepting {
                socket: PathBuf::from("/tmp/deskrinch-session-x/run/wayland-1"),
                last: io::Error::from(io::ErrorKind::ConnectionRefused),
            },
            CompositorLog::new("labwc started\n".into()),
        );
        let said = error.to_string();
        assert!(
            said.starts_with(
                "labwc did not accept a connection on its socket in time (the last try: "
            ),
            "{said}"
        );
        assert!(said.contains("refused"), "{said}");
        assert!(said.ends_with("its log:\nlabwc started\n"), "{said}");
    }
}
