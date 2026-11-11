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
    /// A setting's value is of a type the setting cannot have.
    #[error("the settings portal's {setting} is a {found}, not a {expected}")]
    UnexpectedValue {
        /// The setting: its namespace and key.
        setting: &'static str,
        /// The type it should have.
        expected: &'static str,
        /// The D-Bus signature of what the portal sent.
        found: String,
    },
}
