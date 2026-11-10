//! Why a task could not run to completion.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

/// A task could not finish its work. A check that ran and found problems is
/// not an error: it reports [`crate::Outcome::Failed`].
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// `git` could not be started.
    #[error("could not run `git {args}`")]
    GitSpawn {
        /// The arguments `git` was given.
        args: String,
        /// Why starting it failed.
        source: io::Error,
    },
    /// `git` ran and reported failure.
    #[error("`git {args}` failed ({status}): {stderr}")]
    GitFailed {
        /// The arguments `git` was given.
        args: String,
        /// How it exited.
        status: ExitStatus,
        /// What it printed on standard error, trimmed.
        stderr: String,
    },
    /// `git` listed a path that is not valid UTF-8.
    #[error("`git {args}` listed a path that is not UTF-8")]
    NonUtf8Path {
        /// The arguments `git` was given.
        args: String,
    },
    /// A file could not be read.
    #[error("could not read {}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// Why reading it failed.
        source: io::Error,
    },
}
