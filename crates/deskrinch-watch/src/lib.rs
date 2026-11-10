//! Debounced watching of files and directory trees, which may not exist
//! yet, on a thread of its own.
//!
//! A caller names its [`Target`]s: files (a configuration file), directory
//! trees (an `applications` directory) and directories whose own entries
//! matter (the icons directly in `/usr/share/pixmaps`). The crate decides
//! what to watch so that every way such a path changes is seen:
//!
//! - A file is watched through the directory holding it, not the file
//!   itself, so an editor that saves by writing a new file and renaming it
//!   over the old one is seen, as are creation and removal.
//! - A tree is watched recursively once it exists, and through the
//!   directory holding it; a directory the same way, but not recursively.
//! - A path whose directory does not exist yet is watched through its
//!   nearest existing ancestor, so the directories' creation is seen.
//! - A path that is a symbolic link is watched both where it is written
//!   (the link replaced or re-pointed) and where it points (the target
//!   edited). Directories along the way are resolved, so two spellings of
//!   one directory share one watch.
//!
//! What is watched is planned again after every reported change, as
//! directories come and go.
//!
//! Changes are debounced: after a change that concerns a target, the
//! watcher waits until changes pause for [`Timing::quiet`], but no longer
//! than [`Timing::max_delay`], then reports one [`Event::Changed`]. The
//! subscriber then reads what it watches. Changes that concern no target
//! (another file in the same directory, reading) report nothing.
//!
//! No function needs an async runtime: the subscriber is called on the
//! watcher's thread.
//!
//! # Example
//!
//! ```
//! use deskrinch_watch::{Armed, Event, Target, Timing};
//!
//! # fn main() -> Result<(), deskrinch_watch::WatchError> {
//! # let dir = tempfile::tempdir().map_err(deskrinch_watch::WatchError::Thread)?;
//! let file = dir.path().join("deskrinch/config.toml");
//! // Watch first, then read, then report: nothing is missed in between.
//! let armed = Armed::new(vec![Target::File(file.clone())], Timing::default())?;
//! let contents = std::fs::read_to_string(&file).ok();
//! let watcher = armed.start("config-watch", move |event| match event {
//!     Event::Changed => { /* read the file again */ }
//!     Event::Error(error) => eprintln!("{error}"),
//! })?;
//! # drop((watcher, contents));
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

mod error;
mod plan;
mod worker;

pub use error::{BoxedError, WatchError};
pub use worker::{Armed, Watcher};

/// A path to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A file: its creation, every change, its removal or replacement.
    File(PathBuf),
    /// A directory and everything under it.
    Tree(PathBuf),
    /// A directory's own entries, not what is in its subdirectories.
    Directory(PathBuf),
}

impl Target {
    /// The path watched.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::File(path) | Self::Tree(path) | Self::Directory(path) => path,
        }
    }
}

/// How changes are debounced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long changes must pause before they are reported.
    pub quiet: Duration,
    /// The longest a report waits during a continuous stream of changes.
    pub max_delay: Duration,
}

impl Default for Timing {
    /// A pause of 100 ms, at most 1 s after the first change.
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
        }
    }
}

/// What the watcher reports.
#[derive(Debug)]
pub enum Event {
    /// Something a target holds may have changed: read it again.
    Changed,
    /// Watching failed in part. The watcher carries on; a backend error is
    /// followed by [`Changed`](Self::Changed), since what it missed is
    /// unknown.
    Error(WatchError),
}
