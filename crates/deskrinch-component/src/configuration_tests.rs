//! The configuration against real files in a temporary XDG directory, and
//! its real watcher.

use std::ffi::OsString;
use std::path::Path;

use deskrinch_config::{Section, Settings};
use deskrinch_settings::panel::{Edge, PanelSettings};
use deskrinch_test_wait::wait_for;
use deskrinch_xdg::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::{Configuration, LiveSettings};
use rinch::prelude::*;
use rinch::testing::TestApp;
use std::cell::Cell;
use std::rc::Rc;

/// A second table, beside `[panel]`, as `[general]` will be.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Other {
    accent: String,
}

fn dirs(root: &Path) -> BaseDirs {
    let home: OsString = root.join("home").into();
    let system: OsString = root.join("etc").into();
    BaseDirs::from_vars(|name| match name {
        "XDG_CONFIG_HOME" => Some(home.clone()),
        "XDG_CONFIG_DIRS" => Some(system.clone()),
        _ => None,
    })
}

/// Saves the user's file as an editor does: a new file renamed over it.
fn write(root: &Path, text: &str) {
    let dir = root.join("home/deskrinch");
    std::fs::create_dir_all(&dir).unwrap();
    let new = dir.join("config.toml.new");
    std::fs::write(&new, text).unwrap();
    std::fs::rename(new, dir.join("config.toml")).unwrap();
}

/// The change `live` holds for the UI, once one arrives.
fn next<T: Settings>(live: &LiveSettings<T>) -> T {
    wait_for("a change to be delivered", || {
        live.pending().ok_or("none yet")
    })
}

#[test]
fn two_tables_follow_one_watcher() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "[panel]\nheight = 48\n[other]\naccent = \"red\"\n",
    );
    let configuration = Configuration::start(&dirs(root.path()));
    assert!(format!("{configuration:?}").contains("watched: true"));
    let panel = configuration.follow::<PanelSettings>();
    let other = configuration.follow_section(Section::<Other>::new("other").unwrap());
    assert_eq!(panel.initial.height.get(), 48);
    assert_eq!(other.initial.accent, "red");
    // The tables keep the watcher: the configuration itself can go.
    drop(configuration);

    write(
        root.path(),
        "[panel]\nheight = 48\nedge = \"top\"\n[other]\naccent = \"blue\"\n",
    );
    assert_eq!(next(&panel).edge, Edge::Top);
    assert_eq!(next(&other).accent, "blue");
}

/// The newest settings handed to the UI: a change before the UI exists is
/// what its signal starts with, and a change after reaches it. The signal is
/// made in a `TestApp`'s mount, on this thread, which it hosts, as a
/// hand-over's must be ([`crate::Inbox`]).
#[test]
fn hands_the_newest_settings_to_the_ui() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "[panel]\nheight = 48\n");
    let panel = Configuration::start(&dirs(root.path())).follow::<PanelSettings>();
    write(root.path(), "[panel]\nheight = 64\n");
    assert_eq!(next(&panel).height.get(), 64);
    let held: Rc<Cell<Option<Signal<PanelSettings>>>> = Rc::default();
    let out = Rc::clone(&held);
    let mut app = TestApp::mount(move |scope: &mut RenderScope| {
        out.set(Some(panel.into_signal()));
        scope.create_element("div")
    });
    let signal = held.get().expect("made at the mount");
    assert_eq!(signal.get().height.get(), 64);
    write(root.path(), "[panel]\nheight = 80\n");
    wait_for("the change after the UI exists", || {
        app.settle();
        let height = signal.get().height.get();
        if height == 80 { Ok(()) } else { Err(height) }
    });
}

/// Outside a scope nothing would own the subscription: it would end at
/// once and the signal never follow, so it is refused.
#[test]
#[should_panic(expected = "outside a scope")]
fn into_signal_outside_a_scope_is_refused() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "[panel]\nheight = 48\n");
    let panel = Configuration::start(&dirs(root.path())).follow::<PanelSettings>();
    let _never_followed = panel.into_signal();
}

#[test]
fn a_bad_file_starts_from_the_defaults() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "[panel]\nheight = 0\n");
    let panel = Configuration::start(&dirs(root.path())).follow::<PanelSettings>();
    assert_eq!(panel.initial, PanelSettings::default());
    assert!(format!("{panel:?}").contains("following: true"));
}
