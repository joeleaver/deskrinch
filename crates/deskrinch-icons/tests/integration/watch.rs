//! Following fixture themes as they change.

use std::fs;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use deskrinch_icons::{Event, HICOLOR, Index, Watcher};

use crate::support::{Fixture, found, theme_text};
use deskrinch_test_wait::{quiet_for, recv};

fn watch(f: &Fixture, theme: &str) -> (Watcher, Arc<Index>, Receiver<Event>) {
    let (sender, events) = mpsc::channel();
    let (watcher, index) = Watcher::start(f.search_path(), theme, move |event| {
        sender.send(event).ok();
    })
    .unwrap();
    (watcher, index, events)
}

fn next(events: &Receiver<Event>) -> Arc<Index> {
    match recv("a new index", events) {
        Event::Changed(index) => index,
        Event::Error(error) => panic!("expected a new index, got an error: {error:?}"),
    }
}

/// The first index reported of which `wanted` holds. One change to the
/// files can be reported in steps: a directory made, then the file in it;
/// a file made empty, then written (the fixture writes in place). Each step
/// read in between is a different index, reported in its turn, and only
/// the last is the state the test made.
fn until(events: &Receiver<Event>, wanted: impl Fn(&Index) -> bool) -> Arc<Index> {
    loop {
        let index = next(events);
        if wanted(&index) {
            return index;
        }
    }
}

/// That nothing is reported past the watcher's longest delay.
#[track_caller]
fn nothing(events: &Receiver<Event>) {
    quiet_for(
        "nothing reported",
        events,
        deskrinch_watch::Timing::default().settled(),
    );
}

#[test]
fn follows_icons_added_and_removed() {
    let f = Fixture::new();
    f.theme("b1", HICOLOR, &theme_text("", &["48:48:Fixed"]));
    let (_watcher, index, events) = watch(&f, HICOLOR);
    assert_eq!(found(&index, "new"), None);
    let path = f.icon("b1", HICOLOR, "48", "new.png");
    until(&events, |i| found(i, "new") == Some(path.clone()));
    // The user's copy of the theme, in a base directory with no hicolor yet.
    let user = f.icon("b0", HICOLOR, "48", "new.png");
    until(&events, |i| found(i, "new") == Some(user.clone()));
    fs::remove_file(&user).unwrap();
    until(&events, |i| found(i, "new") == Some(path.clone()));
}

#[test]
fn follows_unthemed_icons() {
    let f = Fixture::new();
    let (_watcher, _, events) = watch(&f, HICOLOR);
    let path = f.icon("pixmaps", "", "", "app.png");
    until(&events, |i| found(i, "app") == Some(path.clone()));
}

#[test]
fn follows_a_new_parent_theme() {
    let f = Fixture::new();
    f.theme("b0", "child", &theme_text("", &["48:48:Fixed"]));
    let (_watcher, index, events) = watch(&f, "child");
    assert_eq!(index.consulted(), ["child", HICOLOR]);
    // Inherit from a theme that is not installed yet, then install it.
    f.theme("b0", "child", &theme_text("parent", &["48:48:Fixed"]));
    until(&events, |i| i.consulted() == ["child", "parent", HICOLOR]);
    f.theme("b1", "parent", &theme_text("", &["48:48:Fixed"]));
    until(&events, |i| i.installed("parent").is_some());
    // The new parent's icons are followed too.
    let path = f.icon("b1", "parent", "48", "from-parent.png");
    until(&events, |i| found(i, "from-parent") == Some(path.clone()));
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
    // Had anything above been delivered, it would arrive before this, and
    // be the next report.
    let path = f.icon("b0", HICOLOR, "48", "real.png");
    assert_eq!(found(&next(&events), "real"), Some(path));
}
