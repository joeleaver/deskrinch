//! The crate's errors.

use std::io;

/// Why following the appearance could not start, or a problem met while
/// following it.
#[derive(Debug, thiserror::Error)]
pub enum AppearanceError {
    /// The bus could not be connected to.
    #[error("cannot connect to the session bus")]
    Connect(#[source] zbus::Error),
    /// The worker thread could not be started.
    #[error("cannot start the appearance thread")]
    Thread(#[source] io::Error),
    /// A call to the bus or the portal failed.
    #[error("the settings portal could not be asked")]
    Call(#[source] zbus::Error),
    /// The portal's colour scheme is not a number.
    #[error("the settings portal's color-scheme is a {found}, not a u32")]
    UnexpectedValue {
        /// The D-Bus signature of what it sent.
        found: String,
    },
}
