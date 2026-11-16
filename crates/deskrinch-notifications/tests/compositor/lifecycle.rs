//! A popup's life: expiry, its sender closing it, replacement in place, and
//! the window going with the last popup.

use deskrinch_test_notify::Signal;
use deskrinch_test_wait::comes_true;

use crate::support::Notifications;

/// The reasons `NotificationClosed` carries.
const EXPIRED: u32 = 1;
const CLOSED: u32 = 3;

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_popup_expires_and_the_window_goes_with_it() {
    let mut notifications = Notifications::start();
    let id = notifications
        .client
        .simple(0, "Brief", "a second and a half", 1500);
    notifications.column("the popup", |c| c.popups.len() == 1);
    assert_eq!(notifications.next_signal(), Signal::Closed(id, EXPIRED));
    notifications.wait_gone();
    let watch = notifications.daemon().watch();
    assert!(
        comes_true(|| watch.windows().iter().all(|w| w.closed)),
        "the window outlived its last popup; its last lines:\n{}",
        watch.tail()
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn its_sender_closes_one_and_the_other_stays() {
    let mut notifications = Notifications::start();
    let first = notifications.client.simple(0, "First", "stays", 0);
    let second = notifications
        .client
        .simple(0, "Second", "closed by its sender", 0);
    notifications.column("two popups", |c| c.popups.len() == 2);
    notifications.client.close(second).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(second, CLOSED));
    notifications.column("one popup", |c| c.popups.len() == 1);
    notifications.client.close(first).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(first, CLOSED));
    notifications.wait_gone();
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_replacement_is_drawn_in_place() {
    let mut notifications = Notifications::start();
    let id = notifications.client.simple(0, "Copying", "one line", 0);
    let short = notifications.column("the popup", |c| c.popups.len() == 1);
    let long_body = "a body\nof three\nlines";
    let replaced = notifications.client.simple(id, "Copying", long_body, 0);
    assert_eq!(replaced, id, "a replacement keeps its id");
    let long = notifications.column("the popup, taller", |c| {
        c.popups.len() == 1 && c.bottom > short.bottom
    });
    assert_eq!(long.top, short.top, "it stays where it was");
    notifications.client.close(id).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(id, CLOSED));
}
