//! What the pointer does to a popup: its close button dismisses it, an
//! action's button and the popup itself invoke actions, and while over it
//! the popup's expiry waits.

use std::collections::HashMap;
use std::time::Duration;

use deskrinch_test_notify::Signal;
use zbus::Variant;

use crate::support::{Notifications, RED_ICON, RED_ICON_NAME, close_button, find};

/// The reasons `NotificationClosed` carries.
const EXPIRED: u32 = 1;
const DISMISSED: u32 = 2;

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_close_button_dismisses() {
    let mut notifications = Notifications::start();
    let id = notifications
        .client
        .simple(0, "Dismiss me", "with its close button", 0);
    let column = notifications.column("the popup", |c| c.popups.len() == 1);
    let image = notifications.picture(&column);
    let (x, y) = close_button(&image, &column, column.popups[0], notifications.scale);
    notifications.click(&column, x, y);
    assert_eq!(notifications.next_signal(), Signal::Closed(id, DISMISSED));
    notifications.wait_gone();
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_action_s_button_is_invoked_and_signalled() {
    let mut notifications = Notifications::start();
    let hints = HashMap::from([("action-icons", Variant::from(true))]);
    let id = notifications.send(
        0,
        "Act",
        "an action drawn as its icon",
        &[RED_ICON_NAME, "Red"],
        &hints,
        0,
    );
    let column = notifications.column("the popup with its action", |c| c.popups.len() == 1);
    let image = notifications.picture(&column);
    let (left, top, right, bottom) =
        find(&image, column.popups[0], RED_ICON).expect("the action's icon");
    notifications.click(&column, left.midpoint(right), top.midpoint(bottom));
    assert_eq!(
        notifications.next_signal(),
        Signal::ActionInvoked(id, RED_ICON_NAME.to_owned())
    );
    assert_eq!(notifications.next_signal(), Signal::Closed(id, DISMISSED));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_click_on_the_popup_invokes_its_default_action() {
    let mut notifications = Notifications::start();
    let id = notifications.send(
        0,
        "Open me",
        "the default action",
        &["default", "Open"],
        &HashMap::new(),
        0,
    );
    let column = notifications.column("the popup", |c| c.popups.len() == 1);
    // The summary: below the header, a third of the way across.
    let (top, bottom) = column.popups[0];
    let x = column.left + (column.right - column.left) / 3;
    let y = top + (bottom - top) / 2;
    notifications.click(&column, x, y);
    assert_eq!(
        notifications.next_signal(),
        Signal::ActionInvoked(id, "default".to_owned())
    );
    assert_eq!(notifications.next_signal(), Signal::Closed(id, DISMISSED));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_pointer_over_a_popup_holds_its_expiry() {
    let mut notifications = Notifications::start();
    // Shown with no timeout, so that a slow first frame is not taken for the
    // expiry; then given one by a replacement, which starts its timer, and
    // the pointer moved onto it, which pauses it. The timeout is long
    // enough for the pointer to be on it first; the wait under the pointer
    // outlasts it.
    let timeout = Duration::from_secs(8);
    let id = notifications
        .client
        .simple(0, "Hold", "under the pointer", 0);
    let column = notifications.column("the popup", |c| c.popups.len() == 1);
    let (top, bottom) = column.popups[0];
    let middle = (column.left.midpoint(column.right), top.midpoint(bottom));
    notifications.client.simple(
        id,
        "Hold",
        "under the pointer",
        i32::try_from(timeout.as_millis()).unwrap(),
    );
    notifications.point(&column, middle.0, middle.1);
    notifications
        .client
        .signals
        .none_within(timeout + Duration::from_secs(2));
    notifications.point_away(&column);
    assert_eq!(notifications.next_signal(), Signal::Closed(id, EXPIRED));
}

/// A popup that moves under a resting pointer, because the one above it
/// closed, is held as one the pointer entered: rinch tells the element now
/// under the pointer that it was entered, with no motion of the pointer.
///
/// It fails until rinch issue 2766 is fixed: rinch sends no enter for an
/// element that moves under a resting pointer
/// (<https://github.com/joeleaver/rinch/issues/2766>).
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_popup_that_moves_under_a_resting_pointer_is_held() {
    let mut notifications = Notifications::start();
    let lower = notifications.client.simple(0, "Lower", "moves up", 0);
    let upper = notifications
        .client
        .simple(0, "Upper", "closed by its sender", 0);
    let column = notifications.column("two popups", |c| c.popups.len() == 2);
    let (top, bottom) = column.popups[0];
    let middle = (column.left.midpoint(column.right), top.midpoint(bottom));
    notifications.point(&column, middle.0, middle.1);
    notifications.client.close(upper).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(upper, 3));
    notifications.column("one popup", |c| c.popups.len() == 1);
    // Now under the resting pointer, given a timeout: held, the
    // replacement keeping the pause, it outlives it.
    notifications
        .client
        .simple(lower, "Lower", "moves up", 1000);
    notifications
        .client
        .signals
        .none_within(Duration::from_secs(3));
    notifications.point_away(&column);
    assert_eq!(notifications.next_signal(), Signal::Closed(lower, EXPIRED));
}
