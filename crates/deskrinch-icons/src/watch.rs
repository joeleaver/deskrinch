//! Following the themes: a new [`Index`] whenever what it was built from
//! changes.
//!
//! Each base directory is watched for its own entries (unthemed icons,
//! themes installed or removed), and each consulted theme's directory in
//! each base directory as a tree, through `deskrinch-watch`. When a change
//! settles the index is built again, and delivered if it differs. When the
//! themes consulted change (an `Inherits` edited, a parent installed), the
//! watches follow.

use std::path::PathBuf;
use std::sync::Arc;

use deskrinch_watch::{Armed, Handle, Target, Timing, WatchError};

use crate::index::Index;
use crate::load::SearchPath;

/// A change to the index.
#[derive(Debug)]
pub enum Event {
    /// The themes changed; this is the index built from them now.
    Changed(Arc<Index>),
    /// Watching failed in part; the watcher carries on and rebuilds.
    Error(WatchError),
}

/// Watches the themes an index is built from and reports [`Event`]s to a
/// subscriber until dropped.
#[derive(Debug)]
pub struct Watcher {
    _watch: deskrinch_watch::Watcher,
}

impl Watcher {
    /// Builds the index for lookups in `theme` and starts watching what it
    /// was built from. `subscriber` is called on the watcher's thread, one
    /// event at a time, with each index that differs from the last.
    ///
    /// # Errors
    ///
    /// [`WatchError`] when the file-watching backend or its thread cannot
    /// be started, or a directory cannot be watched (for example
    /// [`WatchError::WatchLimit`]).
    pub fn start(
        search_path: SearchPath,
        theme: &str,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<(Self, Arc<Index>), WatchError> {
        // Watch the themes first, then load: a change in between is
        // reported rather than lost. Learning which themes to watch reads
        // only their index.theme files; if the full load finds others (an
        // Inherits changed in between), the watches move to them and the
        // index is built again.
        let consulted = Index::consulted_by(&search_path, theme);
        let armed = Armed::new(targets(search_path.dirs(), &consulted), Timing::default())?;
        let handle = armed.handle();
        let mut last = Arc::new(Index::load(&search_path, theme));
        if last.consulted() != consulted {
            handle.set_targets(targets(last.search_path(), last.consulted()));
        }
        let initial = Arc::clone(&last);
        let theme = theme.to_owned();
        let watch = armed.start("icons-watch", move |event| match event {
            deskrinch_watch::Event::Changed => {
                let index = Index::load(&search_path, &theme);
                if index == *last {
                    return;
                }
                retarget(&handle, &last, &index);
                last = Arc::new(index);
                subscriber(Event::Changed(Arc::clone(&last)));
            }
            deskrinch_watch::Event::Error(error) => subscriber(Event::Error(error)),
        })?;
        Ok((Self { _watch: watch }, initial))
    }
}

/// Moves the watches to `newer`'s themes when they are not `older`'s.
fn retarget(handle: &Handle, older: &Index, newer: &Index) {
    if older.consulted() != newer.consulted() {
        handle.set_targets(targets(newer.search_path(), newer.consulted()));
    }
}

/// What an index is built from: each base directory's own entries, and
/// each consulted theme's tree in each base directory.
fn targets(search_path: &[PathBuf], consulted: &[String]) -> Vec<Target> {
    let mut targets = Vec::new();
    for base in search_path {
        targets.push(Target::Directory(base.clone()));
        for theme in consulted {
            targets.push(Target::Tree(base.join(theme)));
        }
    }
    targets
}
