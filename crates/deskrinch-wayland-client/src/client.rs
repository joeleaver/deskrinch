//! [`Client`]: one connection to the compositor, shareable between crates.

use wayland_client::{ConnectError, Connection};

/// A connection to the compositor that several crates may share, each on an
/// event queue and dispatch thread of its own.
///
/// Cloning it clones the handle, not the connection. Everything on the
/// connection shares its fate: a protocol error raised by any user, or the
/// compositor going away, ends it for all of them.
#[derive(Debug, Clone)]
pub struct Client {
    connection: Connection,
}

impl Client {
    /// Connects to the compositor named by the environment
    /// (`WAYLAND_DISPLAY` or `WAYLAND_SOCKET`).
    ///
    /// # Errors
    ///
    /// When no compositor can be reached.
    pub fn connect() -> Result<Self, ConnectError> {
        Connection::connect_to_env().map(Self::from_connection)
    }

    /// A client on an existing connection.
    #[must_use]
    pub fn from_connection(connection: Connection) -> Self {
        Self { connection }
    }

    /// The connection itself.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}
