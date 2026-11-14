//! Menus that cannot be read: each reason the caller is told, and a later
//! failure keeping what was read.

use std::time::Duration;

use deskrinch_status_notifier::{Limits, MenuFailure};

use crate::menus::{Fixture, MENU};
use crate::stand_in_menu::{Entry, Layout};

/// The reason the first read of a menu answering `layout` fails, within
/// `limits`.
fn first_read_fails(limits: Limits, layout: Layout) -> MenuFailure {
    let f = Fixture::start(limits, layout);
    f.handle.open_menu(&f.added.id).unwrap();
    let (item, reason) = f.events.menu_failed();
    assert_eq!(item, f.added.id);
    // The menu is closed: nothing more is read or said.
    assert_eq!(f.handle.menu(&f.added.id), None);
    f.item.layout_updated(MENU);
    f.events.quiet();
    reason
}

fn many(n: i32) -> Layout {
    Layout::Root(Entry::root(
        (1..=n).map(|id| Entry::labelled(id, "x")).collect(),
    ))
}

#[test]
fn a_menu_that_refuses_is_refused() {
    assert_eq!(
        first_read_fails(Limits::default(), Layout::Error),
        MenuFailure::Refused
    );
}

#[test]
fn a_menu_that_does_not_answer_times_out() {
    let limits = Limits {
        read_timeout: Duration::from_millis(300),
        ..Limits::default()
    };
    assert_eq!(
        first_read_fails(limits, Layout::Silent),
        MenuFailure::TimedOut
    );
}

#[test]
fn a_menu_over_its_bounds_is_refused_whole() {
    let per_level = Limits {
        max_menu_entries_per_level: 3,
        ..Limits::default()
    };
    assert_eq!(first_read_fails(per_level, many(4)), MenuFailure::OverBound);
    let deep = Layout::Root(Entry::root(vec![Entry::submenu(
        1,
        "a",
        vec![Entry::submenu(2, "b", vec![Entry::labelled(3, "c")])],
    )]));
    let depth = Limits {
        max_menu_depth: 1,
        ..Limits::default()
    };
    assert_eq!(first_read_fails(depth, deep), MenuFailure::OverBound);
}

#[test]
fn a_reply_over_its_size_is_too_large() {
    let limits = Limits {
        max_menu_reply_bytes: 256,
        ..Limits::default()
    };
    assert_eq!(first_read_fails(limits, many(32)), MenuFailure::TooLarge);
}

#[test]
fn a_later_read_that_fails_keeps_what_was_read() {
    let f = Fixture::start(Limits::default(), many(2));
    let menu = f.open();
    f.item.set_layout(MENU, Layout::Error);
    f.item.layout_updated(MENU);
    f.events.quiet();
    assert_eq!(f.handle.menu(&f.added.id), Some(menu));
}
