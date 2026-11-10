//! The crate's error types.

use std::error::Error as StdError;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::deviation::Deviation;

/// A boxed error from a lower layer (D-Bus), kept as the source of one of
/// this crate's errors.
pub type BoxedError = Box<dyn StdError + Send + Sync>;

/// Why a desktop entry file cannot be used at all. Such a file is skipped
/// and reported in [`crate::Catalog::problems`].
///
/// These are the failures GLib's `GKeyFile` and `GDesktopAppInfo` also
/// refuse. What GLib reads despite the specification is read here too, and
/// recorded as a [`Deviation`](crate::Deviation) instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// A line is neither a comment, a group header nor a `Key=value`, or
    /// its group or key name is malformed.
    #[error("line {line} is malformed")]
    InvalidLine {
        /// The 1-based line number.
        line: usize,
    },
    /// A key comes before the first group header.
    #[error("line {line} has a key outside any group")]
    KeyOutsideGroup {
        /// The 1-based line number.
        line: usize,
    },
    /// The first group is not `[Desktop Entry]`, or there is none.
    #[error("the file does not start with a [Desktop Entry] group")]
    MissingDesktopEntryGroup,
    /// `Type`, or a link's `URL`, is missing.
    #[error("the required key {key} is missing")]
    MissingKey {
        /// The key.
        key: &'static str,
    },
    /// `Type` names a type the specification does not define.
    #[error("unknown entry type {value:?}")]
    UnknownType {
        /// The value of `Type`.
        value: String,
    },
}

/// Why a desktop entry file was skipped during a scan.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScanError {
    /// The file could not be read.
    #[error("cannot read the file")]
    Read(#[source] Arc<io::Error>),
    /// The file is not a valid desktop entry.
    #[error("invalid desktop entry")]
    Parse(#[from] ParseError),
}

/// Something wrong with a file a scan read.
#[derive(Debug, Clone)]
pub struct Problem {
    /// The file.
    pub path: PathBuf,
    /// What is wrong.
    pub kind: ProblemKind,
}

/// How bad a [`Problem`] is.
#[derive(Debug, Clone)]
pub enum ProblemKind {
    /// The file could not be used at all and was skipped.
    Skipped(ScanError),
    /// The file deviates from the specification but was read, the way GLib
    /// reads it: a warning.
    Deviation(Deviation),
}

impl Problem {
    /// Whether the file was skipped, rather than read with a warning.
    #[must_use]
    pub fn is_skipped(&self) -> bool {
        matches!(self.kind, ProblemKind::Skipped(_))
    }
}

/// Why an `Exec` value cannot be turned into a command line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecError {
    /// The command line has no program.
    #[error("the command line is empty")]
    Empty,
    /// A double quote is not closed.
    #[error("a quote is not closed")]
    UnterminatedQuote,
    /// The command line ends in a lone `%`.
    #[error("the command line ends in a lone %")]
    TrailingPercent,
    /// A field code the specification does not define; such a command line
    /// must not be run.
    #[error("unknown field code %{0}")]
    UnknownFieldCode(char),
    /// More than one of `%f`, `%F`, `%u` and `%U`.
    #[error("more than one of %f, %F, %u and %U")]
    MultipleFileCodes,
    /// `%F`, `%U` or `%i` is part of a larger argument.
    #[error("%{0} must be an argument of its own")]
    NotStandalone(char),
    /// The program name contains `=`, which the specification forbids.
    #[error("the program {0:?} contains '='")]
    InvalidProgram(String),
    /// A URI that is not a local file was given to `%f` or `%F`.
    #[error("{0} is not a local file")]
    NotALocalFile(String),
}

/// Why an entry could not be launched.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    /// The entry is a `Link` or `Directory`, not an application.
    #[error("the entry is not an application")]
    NotAnApplication,
    /// The entry has no `Exec` and cannot be activated over D-Bus.
    #[error("the entry has no Exec and cannot be activated over D-Bus")]
    NoCommand,
    /// The entry has no action with this id.
    #[error("the entry has no action {0:?}")]
    UnknownAction(String),
    /// The entry has `Terminal=true` and the context names no terminal.
    #[error("the entry runs in a terminal and no terminal emulator is configured")]
    TerminalRequired,
    /// The `Exec` value is invalid.
    #[error("invalid Exec")]
    Exec(#[from] ExecError),
    /// The process could not be started.
    #[error("cannot start {program}")]
    Spawn {
        /// The program.
        program: String,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The desktop-file ID is not a valid D-Bus name, which
    /// `DBusActivatable` requires.
    #[error("{0} is not a valid D-Bus name")]
    InvalidBusName(String),
    /// D-Bus activation failed.
    #[error("D-Bus activation of {bus_name} failed")]
    Activation {
        /// The application's bus name.
        bus_name: String,
        /// Why.
        #[source]
        source: BoxedError,
    },
}
