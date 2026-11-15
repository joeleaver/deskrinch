//! Connecting to a compositor's Wayland socket: the one way the session
//! and the test client make their connection by path.
//!
//! A socket file can be there before anything accepts on it: a compositor
//! binds its socket and listens on it a moment later, and a compositor that
//! has died leaves its socket file behind. Either way a connection is
//! refused (`ECONNREFUSED`), and the refusal alone does not say which.
//!
//! - [`connect_when_accepting`] is for whoever started the compositor and
//!   can see whether it still runs: it tries again while the socket refuses
//!   (or is not there yet), asking the caller between tries whether to go
//!   on, until a deadline. The waiting between tries, and the look at the
//!   compositor, are the caller's ([`Connecting::Stopped`]), so nothing
//!   here waits on the clock.
//! - [`connect_once`] is for a connection made after the compositor has
//!   been seen accepting: one try, a refusal reported as such.

use std::convert::Infallible;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use deskrinch_test_wait::Deadline;

/// Why no connection was made.
#[derive(Debug, thiserror::Error)]
pub enum Connecting<E = Infallible> {
    /// The caller's look between tries said to stop: its reason (the
    /// compositor exited, an interrupt).
    #[error(transparent)]
    Stopped(E),
    /// The socket refused every try (or was not there) until the deadline.
    #[error("nothing accepted a connection on {} ({last})", socket.display())]
    NotAccepting {
        /// The socket.
        socket: PathBuf,
        /// The last try's error.
        last: io::Error,
    },
    /// A try failed otherwise, which another try would not change.
    #[error("could not connect to {}", socket.display())]
    Failed {
        /// The socket.
        socket: PathBuf,
        /// What failed.
        source: io::Error,
    },
}

/// What one try came to.
enum Try {
    Connected(UnixStream),
    /// Refused, or no socket file yet: a later try may connect.
    NotYet(io::Error),
    Failed(io::Error),
}

fn attempt(socket: &Path) -> Try {
    match UnixStream::connect(socket) {
        Ok(stream) => Try::Connected(stream),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            Try::NotYet(error)
        }
        Err(error) => Try::Failed(error),
    }
}

/// Connects to the Wayland socket at `socket`, trying again while it
/// refuses or is not there yet, until `deadline`.
///
/// It always tries at least once, and tries again only before the
/// deadline. Between two tries it calls `between`, which waits as long as
/// the caller likes and says whether to go on: an `Err` ends the attempt
/// with [`Connecting::Stopped`] (whoever started the compositor returns one
/// when it has exited).
///
/// # Errors
///
/// [`Connecting::Stopped`] with `between`'s reason;
/// [`Connecting::NotAccepting`] when the deadline passed with the socket
/// still refusing; [`Connecting::Failed`] at once for any other error.
pub fn connect_when_accepting<E>(
    socket: &Path,
    deadline: Deadline,
    mut between: impl FnMut() -> Result<(), E>,
) -> Result<UnixStream, Connecting<E>> {
    loop {
        match attempt(socket) {
            Try::Connected(stream) => return Ok(stream),
            Try::Failed(source) => {
                return Err(Connecting::Failed {
                    socket: socket.to_owned(),
                    source,
                });
            }
            Try::NotYet(last) if deadline.passed() => {
                return Err(Connecting::NotAccepting {
                    socket: socket.to_owned(),
                    last,
                });
            }
            Try::NotYet(_) => between().map_err(Connecting::Stopped)?,
        }
    }
}

/// Connects to the Wayland socket at `socket` with one try: for a
/// connection made after the compositor has been seen accepting, when a
/// refusal means it no longer does.
///
/// # Errors
///
/// [`Connecting::NotAccepting`] when the socket refuses or is gone;
/// [`Connecting::Failed`] for any other error.
pub fn connect_once(socket: &Path) -> Result<UnixStream, Connecting> {
    match attempt(socket) {
        Try::Connected(stream) => Ok(stream),
        Try::NotYet(last) => Err(Connecting::NotAccepting {
            socket: socket.to_owned(),
            last,
        }),
        Try::Failed(source) => Err(Connecting::Failed {
            socket: socket.to_owned(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests;
