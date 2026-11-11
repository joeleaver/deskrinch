//! Following fixture themes as they change.

use std::fs;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_icons::{Event, HICOLOR, Index, Watcher};

use crate::support::{Fixture, found, theme_text};

/// How long a test waits for an event before failing.
const PATIENCE: Duration = Duration::from_secs(10);

fn watch(f: &Fixture, theme: &str) -> (Watcher, Arc<Index>, Receiver<Event>) {
    let (sender, events) = mpsc::channel();
    let (watcher, index) = Watcher::start(f.search_path(), theme, move |event| {
        sender.send(event).ok();
    })
    .unwrap();
    (watcher, index, events)
}

fn next(events: &Receiver<Event>) -> Arc<Index> {
    match events.recv_timeout(PATIENCE) {
        Ok(Event::Changed(index)) => index,
        other => panic!("expected a new index, got {other:?}"),
    }
}

fn nothing(events: &Receiver<Event>) {
    if let Ok(event) = events.recv_timeout(Duration::from_millis(1500)) {
        panic!("expected nothing, got {event:?}");
    }
}

#[test]
fn follows_icons_added_and_removed() {
    let f = Fixture::new();
    f.theme("b1", HICOLOR, &theme_text("", &["48:48:Fixed"]));
    let (_watcher, index, events) = watch(&f, HICOLOR);
    assert_eq!(found(&index, "new"), None);
    let path = f.icon("b1", HICOLOR, "48", "new.png");
    assert_eq!(found(&next(&events), "new"), Some(path.clone()));
    // The user's copy of the theme, in a base directory with no hicolor yet.
    let user = f.icon("b0", HICOLOR, "48", "new.png");
    assert_eq!(found(&next(&events), "new"), Some(user.clone()));
    fs::remove_file(&user).unwrap();
    assert_eq!(found(&next(&events), "new"), Some(path));
}

#[test]
fn follows_unthemed_icons() {
    let f = Fixture::new();
    let (_watcher, _, events) = watch(&f, HICOLOR);
    let path = f.icon("pixmaps", "", "", "app.png");
    assert_eq!(found(&next(&events), "app"), Some(path));
}

#[test]
fn follows_a_new_parent_theme() {
    let f = Fixture::new();
    f.theme("b0", "child", &theme_text("", &["48:48:Fixed"]));
    let (_watcher, index, events) = watch(&f, "child");
    assert_eq!(index.consulted(), ["child", HICOLOR]);
    // Inherit from a theme that is not installed yet, then install it.
    f.theme("b0", "child", &theme_text("parent", &["48:48:Fixed"]));
    assert_eq!(next(&events).consulted(), ["child", "parent", HICOLOR]);
    f.theme("b1", "parent", &theme_text("", &["48:48:Fixed"]));
    let installed = next(&events);
    assert!(installed.installed("parent").is_some());
    // The new parent's icons are followed too.
    let path = f.icon("b1", "parent", "48", "from-parent.png");
    let mut index = next(&events);
    // The directory's creation and the file may arrive as two reports.
    if found(&index, "from-parent").is_none() {
        index = next(&events);
    }
    assert_eq!(found(&index, "from-parent"), Some(path));
}

#[test]
fn delivers_nothing_for_changes_that_leave_the_index_alone() {
    let f = Fixture::new();
    f.theme("b0", HICOLOR, &theme_text("", &["48:48:Fixed"]));
    let (_watcher, _, events) = watch(&f, HICOLOR);
    // Not an icon, a theme nobody consults, and a directory the theme does
    // not list.
    f.icon("b0", HICOLOR, "48", "notes.txt");
    f.theme("b1", "unrelated", &theme_text("", &["48:48:Fixed"]));
    f.icon("b0", HICOLOR, "unlisted", "x.png");
    nothing(&events);
    // Had anything above been delivered, it would arrive before this.
    let path = f.icon("b0", HICOLOR, "48", "real.png");
    assert_eq!(found(&next(&events), "real"), Some(path));
}
