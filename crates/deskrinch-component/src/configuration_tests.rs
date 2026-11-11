//! The configuration against real files in a temporary XDG directory, and
//! its real watcher.

use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use deskrinch_config::{Section, Settings};
use deskrinch_settings::panel::{Edge, PanelSettings};
use deskrinch_xdg::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::{Configuration, LiveSettings};

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
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(settings) = live.pending() {
            return settings;
        }
        assert!(Instant::now() < deadline, "no change delivered");
        std::thread::sleep(Duration::from_millis(20));
    }
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

#[test]
fn hands_the_newest_settings_to_the_ui() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "[panel]\nheight = 48\n");
    let panel = Configuration::start(&dirs(root.path())).follow::<PanelSettings>();
    write(root.path(), "[panel]\nheight = 64\n");
    assert_eq!(next(&panel).height.get(), 64);
    // A change before the UI exists is what its signal starts with.
    let signal = panel.into_signal();
    assert_eq!(signal.get().height.get(), 64);
}

#[test]
fn a_bad_file_starts_from_the_defaults() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "[panel]\nheight = 0\n");
    let panel = Configuration::start(&dirs(root.path())).follow::<PanelSettings>();
    assert_eq!(panel.initial, PanelSettings::default());
    assert!(format!("{panel:?}").contains("following: true"));
}
