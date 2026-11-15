//! The test client's connection to a compositor by its socket's path, for
//! code that sets a session up rather than runs in it: one try, through
//! `deskrinch-test-wayland`'s one way of connecting by path. A session
//! hands its socket over only once the compositor has accepted a
//! connection on it, so a refusal here means the compositor no longer
//! accepts, and is said so.

use std::path::Path;

use deskrinch_test_wayland::connect_once;
use wayland_client::Connection;

use crate::Error;

/// A connection to the compositor listening on the Wayland socket at
/// `socket`.
pub(crate) fn connect_at(socket: &Path) -> Result<Connection, Error> {
    let stream = connect_once(socket)?;
    Connection::from_socket(stream).map_err(Error::Connect)
}
