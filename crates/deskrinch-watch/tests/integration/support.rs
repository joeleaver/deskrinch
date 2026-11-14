//! Fixtures shared by the tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_test_wait::recv;
use deskrinch_watch::{Event, Target, Timing, Watcher};
use tempfile::TempDir;

/// A temporary root, resolved (the system's temporary directory may be
/// reached through a link).
pub(crate) struct Root {
    _dir: TempDir,
    pub(crate) path: PathBuf,
}

impl Root {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        Self { _dir: dir, path }
    }

    pub(crate) fn join(&self, relative: &str) -> PathBuf {
        self.path.join(relative)
    }
}

/// Watches `targets`; every report arrives on the receiver.
pub(crate) fn watch(targets: Vec<Target>) -> (Watcher, Receiver<Event>) {
    let (sender, events) = mpsc::channel();
    let watcher = Watcher::start(targets, Timing::default(), "test-watch", move |event| {
        sender.send(event).ok();
    })
    .unwrap();
    (watcher, events)
}

/// Writes `text` to `path` in place, creating its directories.
pub(crate) fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Waits for the next report, which must be a change.
pub(crate) fn changed(events: &Receiver<Event>) {
    match recv("a change", events) {
        Event::Changed => {}
        Event::Error(error) => panic!("expected a change, got an error: {error:?}"),
    }
}

/// Asserts that nothing is reported for a while: long enough for a
/// change to have settled and been reported.
pub(crate) fn quiet(events: &Receiver<Event>) {
    if let Ok(event) = events.recv_timeout(Duration::from_millis(1500)) {
        panic!("expected nothing, got {event:?}");
    }
}
