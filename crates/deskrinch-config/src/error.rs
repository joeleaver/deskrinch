//! The crate's error type. Problems in the files are not errors but
//! [`Diagnostic`](crate::Diagnostic)s: they never stop a component;
//! watching fails with [`deskrinch_watch::WatchError`].

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
