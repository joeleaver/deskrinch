//! The crate's error types.

use std::error::Error as StdError;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

/// A boxed error from a lower layer (D-Bus, the file-watching backend),
/// kept as the source of one of this crate's errors.
pub type BoxedError = Box<dyn StdError + Send + Sync>;

/// Why a desktop entry file is invalid. An invalid file is skipped, as the
/// specification requires, and reported in [`crate::Catalog::problems`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The file is not UTF-8.
    #[error("the file is not UTF-8")]
    NotUtf8,
    /// A line is neither a comment, a group header nor a `Key=value`.
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
    /// A group appears twice.
    #[error("group [{group}] appears twice")]
    DuplicateGroup {
        /// The group's name.
        group: String,
    },
    /// A key appears twice in a group.
    #[error("key {key} appears twice in [{group}]")]
    DuplicateKey {
        /// The group's name.
        group: String,
        /// The key.
        key: String,
    },
    /// A boolean key is neither `true` nor `false`.
    #[error("key {key} is {value:?}, not true or false")]
    InvalidBoolean {
        /// The key.
        key: String,
        /// Its value.
        value: String,
    },
    /// There is no `[Desktop Entry]` group.
    #[error("there is no [Desktop Entry] group")]
    MissingDesktopEntryGroup,
    /// A key the entry's type requires is missing.
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

/// A file a scan skipped, and why.
#[derive(Debug, Clone)]
pub struct Problem {
    /// The file.
    pub path: PathBuf,
    /// Why it was skipped.
    pub error: ScanError,
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

/// Why watching the data directories failed.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The file-watching backend failed.
    #[error("the file-watching backend failed")]
    Backend(#[source] BoxedError),
    /// The worker thread could not be started.
    #[error("cannot start the watcher thread")]
    Thread(#[source] io::Error),
}
