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
use crate::support::{Column, Notifications, reddish, whole};

/// The reasons `NotificationClosed` carries.
const EXPIRED: u32 = 1;
const CLOSED: u32 = 3;

/// A body of three lines.
const THREE: &str = "line 1\nline 2\nline 3";

/// A body of six lines, the most a popup shows.
const SIX: &str = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";

/// The timeout a waiting notification is given, in milliseconds: short, so
/// that a hold that failed shows well inside [`HELD_FOR`].
const TIMEOUT_MS: i32 = 1500;

/// How long a held notification is watched for not expiring: twice its
/// timeout, so that a hold that failed (it would expire 1.5 s after the
/// replacement that gave it the timeout) shows with 1.5 s to spare on a
/// loaded runner. The hold is in force before the timeout is given: the
/// notification already waits when it is replaced.
const HELD_FOR: Duration = Duration::from_secs(3);

/// The timeout a held notification is given before it is shown again and
/// the screen is waited for, in milliseconds: longer than that wait (a
/// still screen, a second or two) takes, so that it is shown when the
/// screen is read, and expires after.
const SHOWN_AGAIN_TIMEOUT_MS: i32 = 6000;

/// Asserts that every popup of `column` is drawn whole.
fn assert_whole(notifications: &mut Notifications, column: &Column) {
    let image = notifications.picture(column);
    for (at, popup) in column.popups.iter().enumerate() {
        assert!(
            whole(&image, column, *popup, notifications.scale),
            "popup {at} is cut: {column:?}"
        );
    }
}

/// Asserts that every popup of the settled column `shown`/`waiting` is
/// drawn whole.
fn all_whole(notifications: &mut Notifications, shown: usize, waiting: usize) {
    let column = notifications.settled(shown, waiting);
    assert_whole(notifications, &column);
}

/// The fit the daemon settles on for `live` notifications, said after its
/// line `since` (a mark taken before the change that moves it), read from
/// its lines, once the screen agrees and every shown popup is drawn whole.
fn settle(notifications: &mut Notifications, since: usize, live: usize) -> (usize, usize) {
    let (shown, waiting) = notifications.fit_drawn_since(since, live);
    all_whole(notifications, shown, waiting);
    (shown, waiting)
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn more_than_fit_the_shown_whole_the_rest_counted_and_held() {
    let mut notifications = Notifications::start();
    let mark = notifications.mark();
    // The oldest, last in the column's order: the first to wait.
    let oldest = notifications.client.simple(0, "Oldest", THREE, 0);
    let others: Vec<u32> = (1..=9)
        .map(|n| {
            notifications
                .client
                .simple(0, &format!("Newer {n}"), THREE, 0)
        })
        .collect();
    let (shown, waiting) = settle(&mut notifications, mark, 10);
    assert!(
        waiting > 0,
        "{shown} shown: ten three-line popups fit the room"
    );
    // Waiting, given a timeout: it is held, the replacement keeping the
    // pause, and outlives it.
    notifications
        .client
        .simple(oldest, "Oldest", THREE, TIMEOUT_MS);
    notifications.client.signals.none_within(HELD_FOR);
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
    let mark = notifications.mark();
    for n in 1..=10 {
        notifications
            .client
            .simple(0, &format!("Normal {n}"), THREE, 0);
    }
    let (shown, waiting) = settle(&mut notifications, mark, 10);
    assert!(waiting > 0, "{shown} shown: the column is not full");
    let critical = std::collections::HashMap::from([("urgency", Variant::from(2_u8))]);
    notifications.send(0, "Critical", "on top", &[], &critical, 0);
    let (shown_after, waiting_after) = notifications.fit_drawn(11);
    assert!(
        waiting_after >= waiting,
        "a popup came back for a critical one"
    );
    let column = notifications.settled(shown_after, waiting_after);
    assert_whole(&mut notifications, &column);
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
    // A full column: all fit with no room to spare.
    let (live, column) = notifications.full_column();
    assert_whole(&mut notifications, &column);
    let oldest = live[0];
    // The tallest panel there is, along the top: the room shrinks by more
    // than a popup's height. The full column's own fit counts the same
    // notifications, so only a fit said after the panel came counts.
    let mark = notifications.mark();
    let panel = Panel::along_the_top(256);
    let (_, waiting) = settle(&mut notifications, mark, live.len());
    assert!(waiting > 0, "the room shrank and all still fit");
    // The oldest, last in the column's order, waits, held: given a
    // timeout, it outlives it.
    notifications
        .client
        .simple(oldest, "Filling", "", TIMEOUT_MS);
    notifications.client.signals.none_within(HELD_FOR);
    // Still held, given a timeout long enough for the screen to be read
    // once it is shown again: the room comes back, all are shown whole,
    // and its timeout then runs.
    notifications
        .client
        .simple(oldest, "Filling", "", SHOWN_AGAIN_TIMEOUT_MS);
    drop(panel);
    all_whole(&mut notifications, live.len(), 0);
    assert_eq!(notifications.next_signal(), Signal::Closed(oldest, EXPIRED));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_first_popup_taller_than_the_room_is_never_drawn_cut() {
    let mut notifications = Notifications::start();
    // The tallest panel there is: the room left is less than a six-line
    // popup where the output is 360 logical pixels high (scale 2).
    let _panel = Panel::along_the_top(256);
    let mark = notifications.mark();
    notifications.client.simple(0, "Tall", SIX, 0);
    let (shown, waiting) = settle(&mut notifications, mark, 1);
    if notifications.scale >= 2 {
        assert_eq!(
            (shown, waiting),
            (0, 1),
            "a popup taller than the room shown"
        );
    }
}
