//! The column when not all popups fit its room: those shown are whole, the
//! rest wait (paused) and are counted, the critical ones come first, and
//! the room follows what panels reserve.
//!
//! The counts are the daemon's own (`showing N, M waiting`, at info level),
//! with the screen held to them ([`Notifications::settled`]): the shown
//! popups drawn, the count line there exactly while some wait.

use std::time::Duration;

use deskrinch_test_notify::Signal;
use zbus::Variant;

use crate::panel::Panel;
use crate::support::{Notifications, reddish, whole};

/// The reasons `NotificationClosed` carries.
const EXPIRED: u32 = 1;
const CLOSED: u32 = 3;

/// A body of three lines.
const THREE: &str = "line 1\nline 2\nline 3";

/// A body of six lines, the most a popup shows.
const SIX: &str = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";

/// Asserts that every popup of the settled column `shown`/`waiting` is
/// drawn whole, and returns how many are shown.
fn all_whole(notifications: &mut Notifications, shown: usize, waiting: usize) -> usize {
    let column = notifications.settled(shown, waiting);
    let image = notifications.picture(&column);
    for (at, popup) in column.popups.iter().enumerate() {
        assert!(
            whole(&image, &column, *popup, notifications.scale),
            "popup {at} is cut: {column:?}"
        );
    }
    column.popups.len()
}

/// The fit once the column has settled on what the daemon says.
fn settle(notifications: &mut Notifications) -> (usize, usize) {
    notifications.column("the column", |_| true);
    let (shown, waiting) = notifications.fit().expect("a fit said");
    all_whole(notifications, shown, waiting);
    (shown, waiting)
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn more_than_fit_the_shown_whole_the_rest_counted_and_held() {
    let mut notifications = Notifications::start();
    // The oldest, last in the column's order: the first to wait.
    let oldest = notifications.client.simple(0, "Oldest", THREE, 0);
    let others: Vec<u32> = (1..=9)
        .map(|n| {
            notifications
                .client
                .simple(0, &format!("Newer {n}"), THREE, 0)
        })
        .collect();
    let (shown, waiting) = settle(&mut notifications);
    assert_eq!(shown + waiting, 10);
    assert!(waiting > 0, "ten three-line popups fit the room");
    // Waiting, given a timeout: it is held, the replacement keeping the
    // pause, and outlives it.
    notifications.client.simple(oldest, "Oldest", THREE, 2000);
    notifications
        .client
        .signals
        .none_within(Duration::from_secs(4));
    // Room freed: once shown, its timeout runs.
    for id in &others {
        notifications.client.close(*id).unwrap();
        assert_eq!(notifications.next_signal(), Signal::Closed(*id, CLOSED));
    }
    assert_eq!(notifications.next_signal(), Signal::Closed(oldest, EXPIRED));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_critical_one_into_a_full_column_goes_on_top() {
    let mut notifications = Notifications::start();
    for n in 1..=10 {
        notifications
            .client
            .simple(0, &format!("Normal {n}"), THREE, 0);
    }
    let (shown, waiting) = settle(&mut notifications);
    assert!(waiting > 0, "{shown} shown: the column is not full");
    let critical = std::collections::HashMap::from([("urgency", Variant::from(2_u8))]);
    notifications.send(0, "Critical", "on top", &[], &critical, 0);
    let (shown_after, waiting_after) = settle(&mut notifications);
    assert_eq!(shown_after + waiting_after, 11);
    assert!(
        waiting_after >= waiting,
        "a popup came back for a critical one"
    );
    let column = notifications.settled(shown_after, waiting_after);
    let image = notifications.picture(&column);
    let (first, last) = column.popups[0];
    let red = (first..=last)
        .any(|y| (column.left..=column.right).any(|x| image.pixel(x, y).is_some_and(reddish)));
    assert!(red, "the critical one is not on top: {column:?}");
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_room_shrinking_holds_a_shown_one_back_and_its_return_shows_it() {
    let mut notifications = Notifications::start();
    // A full column: popups until one waits, then the newest closed, so
    // that all fit with no room to spare.
    let oldest = notifications.client.simple(0, "Oldest", "", 0);
    let mut live = 1;
    loop {
        let newest = notifications
            .client
            .simple(0, &format!("Newer {live}"), "", 0);
        live += 1;
        let (_, waiting) = settle(&mut notifications);
        if waiting > 0 {
            notifications.client.close(newest).unwrap();
            assert_eq!(notifications.next_signal(), Signal::Closed(newest, CLOSED));
            live -= 1;
            break;
        }
    }
    all_whole(&mut notifications, live, 0);
    // The tallest panel there is, along the top: the room shrinks by more
    // than a popup's height.
    let panel = Panel::along_the_top(256);
    let (shown, waiting) = settle(&mut notifications);
    assert!(waiting > 0, "the room shrank and all still fit");
    assert_eq!(shown + waiting, live);
    // The oldest, last in the column's order, waits, held: given a
    // timeout, it outlives it.
    notifications.client.simple(oldest, "Oldest", "", 2000);
    notifications
        .client
        .signals
        .none_within(Duration::from_secs(4));
    drop(panel);
    all_whole(&mut notifications, live, 0);
    assert_eq!(notifications.next_signal(), Signal::Closed(oldest, EXPIRED));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_first_popup_taller_than_the_room_is_never_drawn_cut() {
    let mut notifications = Notifications::start();
    // The tallest panel there is: the room left is less than a six-line
    // popup where the output is 360 logical pixels high (scale 2).
    let _panel = Panel::along_the_top(256);
    notifications.client.simple(0, "Tall", SIX, 0);
    let (shown, waiting) = settle(&mut notifications);
    assert_eq!(shown + waiting, 1);
    if notifications.scale >= 2 {
        assert_eq!(
            (shown, waiting),
            (0, 1),
            "a popup taller than the room shown"
        );
    }
}
