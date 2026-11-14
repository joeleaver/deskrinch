//! Fixtures shared by the tests.

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_config::{Diagnostic, Event, Section, Settings, Sources, Subscription, Watcher};
use deskrinch_test_wait::PATIENCE;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

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
    pub(crate) fn watch(&self) -> (Watched, Panel, Receiver<Event<Panel>>) {
        let watcher = Watcher::start(self.sources(), |error| panic!("{error}")).unwrap();
        let (subscription, panel, events) = subscribe(&watcher, "panel");
        (
            Watched {
                _subscription: subscription,
                _watcher: watcher,
            },
            panel,
            events,
        )
    }
}

/// A watcher and its one subscription, kept alive together.
pub(crate) struct Watched {
    _subscription: Subscription,
    _watcher: Watcher,
}

/// Subscribes to section `name` as a `T`: the subscription, the initial
/// settings and the events.
pub(crate) fn subscribe<T: Settings>(
    watcher: &Watcher,
    name: &str,
) -> (Subscription, T, Receiver<Event<T>>) {
    let (sender, events) = mpsc::channel();
    let section = Section::new(name).unwrap();
    let (subscription, resolved) = watcher.subscribe(section, move |event| {
        sender.send(event).ok();
    });
    (subscription, resolved.settings, events)
}

/// Settings several components share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct General {
    pub(crate) terminal: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            terminal: "foot".into(),
        }
    }
}

/// Writes `text` to `path` in place, creating its directories.
pub(crate) fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The next event, which must come soon.
fn next<T>(events: &Receiver<Event<T>>) -> Event<T> {
    events.recv_timeout(PATIENCE).expect("no event arrived")
}

/// The next event, which must be new settings.
pub(crate) fn next_change<T: Debug>(events: &Receiver<Event<T>>) -> T {
    match next(events) {
        Event::Changed(settings) => settings,
        Event::Diagnostics(diagnostics) => {
            panic!("expected new settings, got diagnostics {diagnostics:?}")
        }
    }
}

/// Asserts that nothing arrives for long enough for a change to have
/// settled and been delivered.
pub(crate) fn nothing<T: Debug>(events: &Receiver<Event<T>>) {
    if let Ok(event) = events.recv_timeout(Duration::from_millis(1500)) {
        panic!("expected nothing, got {event:?}");
    }
}

/// The next event, which must be diagnostics.
pub(crate) fn next_diagnostics<T: Debug>(events: &Receiver<Event<T>>) -> Vec<Diagnostic> {
    match next(events) {
        Event::Diagnostics(diagnostics) => diagnostics,
        Event::Changed(settings) => panic!("expected diagnostics, got settings {settings:?}"),
    }
}
