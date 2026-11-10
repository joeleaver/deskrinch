//! The crate's error types.

use std::io;
use std::path::PathBuf;

/// Why the time zone the environment names could not be used. The clock
/// then keeps UTC, as glibc does, until the zone becomes readable.
#[derive(Debug, thiserror::Error)]
pub enum TimeZoneError {
    /// The time zone file could not be read.
    #[error("cannot read the time zone file {}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The time zone file is not a valid TZif file.
    #[error("{} is not a valid time zone file", path.display())]
    InvalidFile {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: jiff::Error,
    },
    /// `TZ` is neither a time zone file nor a valid POSIX rule.
    #[error("TZ={rule:?} is neither a time zone file nor a POSIX rule")]
    InvalidRule {
        /// The value of `TZ`.
        rule: String,
        /// Why it is not a rule.
        #[source]
        source: jiff::Error,
    },
}

/// Why a [`Clock`](crate::Clock) could not start, or a problem it met while
/// running.
#[derive(Debug, thiserror::Error)]
pub enum ClockError {
    /// The real-time timer could not be created or armed.
    #[error("the real-time timer failed")]
    Timer(#[source] io::Error),
    /// Watching the time zone file failed: changes to it may be missed.
    #[error("cannot watch the time zone file")]
    Watch(#[source] io::Error),
    /// The worker thread could not be started.
    #[error("cannot start the clock thread")]
    Thread(#[source] io::Error),
    /// The time zone could not be read; the clock shows UTC.
    #[error("the time zone is unusable")]
    TimeZone(#[from] TimeZoneError),
}

/// Why a [`Formatter`](crate::Formatter) could not be made.
#[derive(Debug, thiserror::Error)]
#[error("cannot load the date and time patterns")]
pub struct FormatError(#[from] icu_datetime::DateTimeFormatterLoadError);
