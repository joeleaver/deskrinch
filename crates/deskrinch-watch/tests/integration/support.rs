//! Fixtures shared by the tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};

use deskrinch_test_wait::{quiet_for, recv};
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

/// Waits for what the writes just made to be reported: a change, then
/// reports stopping. However the writes were spread in time (one report,
/// or a burst split in two on a slow machine), it returns once nothing has
/// come for [`Timing::settled`] after the last report, so a later claim
/// starts from a watcher with nothing left to say. It fails only on an
/// error, or on no change within the patience.
pub(crate) fn reported(events: &Receiver<Event>) {
    changed(events);
    loop {
        match events.recv_timeout(Timing::default().settled()) {
            Ok(Event::Changed) => {}
            Ok(Event::Error(error)) => panic!("expected changes, got an error: {error:?}"),
            Err(RecvTimeoutError::Timeout) => return,
            Err(RecvTimeoutError::Disconnected) => panic!("the watcher stopped"),
        }
    }
}

/// Asserts that nothing is reported for long enough for a change to have
/// settled and been reported: past the watcher's longest delay.
///
/// A quiet window, kept where no ordered check can stand in for it: an
/// [`Event::Changed`] carries nothing that tells which write it reports, so
/// a later write's report could not be told from a wrong one. It cannot
/// fail because the machine is slow: it starts after the writes are made
/// and their reports have stopped ([`reported`]), and a slow machine only
/// delays a wrong report into it.
#[track_caller]
pub(crate) fn quiet(events: &Receiver<Event>) {
    quiet_for("nothing reported", events, Timing::default().settled());
}
