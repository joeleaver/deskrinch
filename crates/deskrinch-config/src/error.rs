//! The crate's error types. Problems in the files are not errors but
//! [`Diagnostic`](crate::Diagnostic)s: they never stop a component.

use std::error::Error as StdError;
use std::io;

/// A boxed error from the file-watching backend.
pub type BoxedError = Box<dyn StdError + Send + Sync>;

/// A settings type's defaults are unusable. This is a bug in the type, not
/// in any file.
#[derive(Debug, thiserror::Error)]
pub enum DefaultsError {
    /// `T::default()` cannot be written as TOML.
    #[error("the defaults of [{section}] cannot be written as TOML")]
    Serialize {
        /// The section.
        section: String,
        /// Why.
        #[source]
        source: toml::ser::Error,
    },
    /// `T::default()` is not written as a table (it is a number, a string
    /// or a sequence).
    #[error("the defaults of [{section}] are not a table")]
    NotATable {
        /// The section.
        section: String,
    },
    /// `T::default()`, written as TOML, does not read back as a `T`.
    #[error("the defaults of [{section}] do not read back: {message}")]
    RoundTrip {
        /// The section.
        section: String,
        /// Why.
        message: String,
    },
}

/// Why watching the configuration failed.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The settings type's defaults are unusable.
    #[error(transparent)]
    Defaults(#[from] DefaultsError),
    /// The file-watching backend failed.
    #[error("the file-watching backend failed")]
    Backend(#[source] BoxedError),
    /// The worker thread could not be started.
    #[error("cannot start the watcher thread")]
    Thread(#[source] io::Error),
}
