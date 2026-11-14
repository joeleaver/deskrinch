//! What was wrong with the themes an index was built from. None of it stops
//! a lookup: a broken theme is skipped, a broken directory is left out,
//! and the rest is used.

use std::io;
use std::path::PathBuf;

use deskrinch_keyfile::{Deviation, SyntaxError};

/// Something wrong with a theme, found while building an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The theme, by its directory name.
    pub theme: String,
    /// The file or directory concerned, when there is one.
    pub path: Option<PathBuf>,
    /// What is wrong, and what was made of it.
    pub kind: ProblemKind,
}

/// What is wrong with a theme.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProblemKind {
    /// No base directory has the theme's `index.theme`. Lookups go on to
    /// the next theme: its other parents, then `hicolor`.
    #[error("the theme is not installed (no index.theme in any base directory)")]
    Missing,
    /// The name, from `Inherits` or the caller, is not a directory name
    /// (empty, or with a `/`, or `.` or `..`). It is treated as missing.
    #[error("the theme name is not a directory name")]
    InvalidName,
    /// A file or directory exists but cannot be read. An unreadable
    /// `index.theme` is skipped for the next base directory's; an
    /// unreadable icon directory contributes no icons.
    #[error("cannot read it: {message}")]
    Unreadable {
        /// The kind of I/O error.
        kind: io::ErrorKind,
        /// The I/O error's description.
        message: String,
    },
    /// `index.theme` is not a key file GLib would read. The theme is
    /// skipped, as GTK skips it.
    #[error("index.theme is malformed")]
    Syntax(#[source] SyntaxError),
    /// `index.theme` has no `[Icon Theme]` group. The theme is skipped.
    #[error("index.theme has no [Icon Theme] group")]
    NotAnIconTheme,
    /// `index.theme` breaks the key-file format in a way GLib reads
    /// anyway. It is read the same way.
    #[error(transparent)]
    KeyFile(#[from] Deviation),
    /// `Directories` lists a directory with no group of its own. It is left
    /// out.
    #[error("directory {directory} is listed but has no group")]
    DirectoryWithoutGroup {
        /// The directory, relative to the theme.
        directory: String,
    },
    /// A directory's `key` (`Size`, which is required, or `Scale`) is
    /// missing, not a number, or not at least 1. The directory is left out.
    #[error("directory {directory} has no usable {key}")]
    InvalidDirectory {
        /// The directory, relative to the theme.
        directory: String,
        /// The key.
        key: &'static str,
    },
    /// A directory's `Type` is none of `Fixed`, `Scalable` and `Threshold`.
    /// It is read as `Threshold`, the default.
    #[error("directory {directory} has an unknown Type {value:?}, read as Threshold")]
    UnknownType {
        /// The directory, relative to the theme.
        directory: String,
        /// The value of `Type`.
        value: String,
    },
    /// The theme inherits from itself through `cycle` (each theme
    /// inheriting from the next, the last from the first). Lookups visit
    /// each theme once.
    #[error("Inherits forms a cycle: {}", cycle.join(" → "))]
    InheritanceCycle {
        /// The themes on the cycle, from this one.
        cycle: Vec<String>,
    },
    /// The theme has more of something than the index's compact form can
    /// hold: more than 65,536 directories, or base directories past the
    /// 16,384th, or more than `u32::MAX` files or bytes of icon names.
    /// What is past the limit is left out of the index.
    #[error("more {what} than the index holds ({limit}); those past it are left out")]
    TooLarge {
        /// What there are too many of.
        what: &'static str,
        /// How many the index holds.
        limit: u64,
    },
}

impl ProblemKind {
    pub(crate) fn unreadable(error: &io::Error) -> Self {
        Self::Unreadable {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}
