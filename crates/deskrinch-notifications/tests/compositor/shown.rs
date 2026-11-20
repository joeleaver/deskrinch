//! Where popups are shown, and in what order.

use std::collections::HashMap;

use deskrinch_shell::NOTIFICATIONS_UP_LINE;
use deskrinch_test_wait::comes_true;
use zbus::Variant;

use crate::support::{Notifications, reddish};

/// What the daemon has said so far, line by line.
fn lines(notifications: &Notifications) -> Vec<String> {
    notifications.daemon().watch().lines()
}

/// The daemon has made no window by the time it is up and has answered on
/// the bus, and makes one, at the top right, for its first notification.
/// (That it makes none later while idle is [`crate::frames`]' and
/// [`crate::lifecycle`]'s: the window goes with the last popup.)
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn no_window_until_a_notification_then_one_at_the_top_right() {
    let mut notifications = Notifications::start();
    // Bounded by the daemon's own word and an answer of its loop: its up
    // line is said as its root is built, which is where the column's window
    // is declared, and an answer on the bus comes only once its loop has
    // run since; no window opened before then.
    assert!(
        comes_true(|| lines(&notifications)
            .iter()
            .any(|line| line.contains(NOTIFICATIONS_UP_LINE))),
        "the daemon never said it was up; its last lines:\n{}",
        notifications.daemon().watch().tail()
    );
    zbus::block_on(notifications.client.proxy.get_server_information()).unwrap();
    let before = lines(&notifications);
    assert!(
        !before.iter().any(|line| line.contains(" opened:")),
        "a window with nothing to show; its lines:\n{}",
        before.join("\n")
    );
    let sent_at = before.len();
    notifications.client.simple(0, "Hello", "a body", 0);
    let column = notifications.column("one popup", |c| c.popups.len() == 1);
    assert!(
        notifications.top_right(&column),
        "the popup is not at the top right: {column:?}"
    );
    // One window in all, opened after the notification was sent.
    assert_eq!(notifications.daemon().watch().windows().len(), 1);
    let opened = lines(&notifications)
        .iter()
        .position(|line| line.contains(" opened:"));
    assert!(
        opened.is_some_and(|at| at >= sent_at),
        "the window opened before the notification was sent (line {opened:?} of {sent_at})"
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn popups_stack_the_newest_on_top() {
    let mut notifications = Notifications::start();
    notifications.client.simple(0, "Older", "normal", 0);
    notifications.column("one popup", |c| c.popups.len() == 1);
    let critical = HashMap::from([("urgency", Variant::from(2_u8))]);
    notifications.send(0, "Newer", "critical", &[], &critical, 0);
    let column = notifications.column("two popups", |c| c.popups.len() == 2);
    assert!(notifications.top_right(&column), "{column:?}");
    let image = &column.image;
    let red_in = |(first, last): (u32, u32)| {
        (first..=last)
            .any(|y| (column.left..=column.right).any(|x| image.pixel(x, y).is_some_and(reddish)))
    };
    assert!(
        red_in(column.popups[0]),
        "the newer, critical one is not on top"
    );
    assert!(!red_in(column.popups[1]), "the older one has a red border");
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_burst_is_all_shown() {
    let mut notifications = Notifications::start();
    // Summaries alone: three fit an output 360 logical pixels high (the
    // scale-2 pass), where three with bodies would scroll.
    for n in 1..=3 {
        notifications
            .client
            .simple(0, &format!("Burst {n} of three"), "", 0);
    }
    let column = notifications.column("three popups", |c| c.popups.len() == 3);
    assert!(notifications.top_right(&column), "{column:?}");
}
