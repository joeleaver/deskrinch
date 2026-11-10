//! Fixtures shared by the tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_config::{Diagnostic, Event, Sources, Watcher};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

/// How long a test waits for an event before failing.
const PATIENCE: Duration = Duration::from_secs(10);

/// A component's settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Panel {
    pub(crate) height: u32,
    pub(crate) edge: String,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            height: 32,
            edge: "top".into(),
        }
    }
}

/// The default panel at another height.
pub(crate) fn panel(height: u32) -> Panel {
    Panel {
        height,
        ..Panel::default()
    }
}

/// A system and a user configuration file under a temporary root; neither
/// exists until written.
pub(crate) struct Dirs {
    pub(crate) root: TempDir,
}

impl Dirs {
    pub(crate) fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    pub(crate) fn system(&self) -> PathBuf {
        self.root.path().join("etc/xdg/deskrinch/config.toml")
    }

    pub(crate) fn user(&self) -> PathBuf {
        self.root.path().join("home/.config/deskrinch/config.toml")
    }

    pub(crate) fn sources(&self) -> Sources {
        Sources::from_files(vec![self.system()], Some(self.user()))
    }

    /// Watches `[panel]`: the watcher, the initial settings and the events.
    pub(crate) fn watch(&self) -> (Watcher, Panel, Receiver<Event<Panel>>) {
        let (sender, events) = mpsc::channel();
        let (watcher, resolved) = Watcher::start(self.sources(), "panel", move |event| {
            sender.send(event).ok();
        })
        .unwrap();
        (watcher, resolved.settings, events)
    }
}

/// Writes `text` to `path` in place, creating its directories.
pub(crate) fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The next event, which must come soon.
fn next(events: &Receiver<Event<Panel>>) -> Event<Panel> {
    events.recv_timeout(PATIENCE).expect("no event arrived")
}

/// The next event, which must be new settings.
pub(crate) fn next_change(events: &Receiver<Event<Panel>>) -> Panel {
    match next(events) {
        Event::Changed(panel) => panel,
        other => panic!("expected new settings, got {other:?}"),
    }
}

/// The next event, which must be diagnostics.
pub(crate) fn next_diagnostics(events: &Receiver<Event<Panel>>) -> Vec<Diagnostic> {
    match next(events) {
        Event::Diagnostics(diagnostics) => diagnostics,
        other => panic!("expected diagnostics, got {other:?}"),
    }
}
