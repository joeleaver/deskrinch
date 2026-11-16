//! What a change costs in frames: a notification shown, replaced or closed,
//! the waiting set changing with it included, is drawn in exactly one
//! presented frame, and nothing is drawn while nothing
//! changes.
//!
//! A frame of rinch's cached pixels draws nothing and is not counted
//! ([`deskrinch_test_component::drew`]): the software renderer skips its
//! present, and the GPU renderer presents the same pixels again after a
//! change, which this daemon does nothing to ask for.

use std::time::Duration;

use deskrinch_test_component::drew;
use deskrinch_test_notify::Signal;
use deskrinch_test_wait::comes_true;

use crate::support::Notifications;

/// How long the daemon is watched for a frame it must not draw: a frame
/// asked for by a change comes within a few of the compositor's refreshes.
const QUIET: Duration = Duration::from_secs(2);

/// The frames that drew something in `lines` from line `from` on.
fn presented(lines: &[String], from: usize) -> usize {
    lines
        .get(from..)
        .unwrap_or_default()
        .iter()
        .filter(|line| drew(line))
        .count()
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_change_is_one_frame_and_nothing_changing_is_none() {
    let mut notifications = Notifications::start();
    let id = notifications.client.simple(0, "Frames", "counted", 0);
    notifications.column("the popup", |c| c.popups.len() == 1);

    let mark = notifications.daemon().watch().lines().len();
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        0,
        "frames while idle:\n{}",
        notifications.daemon().watch().tail()
    );

    let replaced = notifications
        .client
        .simple(id, "Frames", "counted again", 0);
    assert_eq!(replaced, id);
    // The column's window is not the app's main one (a windowless app has
    // none), so its frames are counted from the lines.
    assert!(comes_true(|| presented(
        &notifications.daemon().watch().lines(),
        mark
    ) >= 1));
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        1,
        "a replacement:\n{}",
        notifications.daemon().watch().tail()
    );

    let mark = notifications.daemon().watch().lines().len();
    let second = notifications.client.simple(0, "Another", "below", 0);
    notifications.column("two popups", |c| c.popups.len() == 2);
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        1,
        "a second popup:\n{}",
        notifications.daemon().watch().tail()
    );

    let mark = notifications.daemon().watch().lines().len();
    notifications.client.close(second).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(second, 3));
    notifications.column("one popup", |c| c.popups.len() == 1);
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        1,
        "a popup closed:\n{}",
        notifications.daemon().watch().tail()
    );
}

/// The waiting set changing costs a frame like any other change: a popup
/// into a full column (one more waits, the count line appears), and a
/// close that brings a waiting one back.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_waiting_set_changing_is_one_frame() {
    let mut notifications = Notifications::start();
    // A full column: popups until one waits, then the newest closed.
    let mut live = Vec::new();
    loop {
        live.push(notifications.client.simple(0, "Filling", "", 0));
        notifications.column("the column", |_| true);
        let (shown, waiting) = notifications.fit().expect("a fit said");
        notifications.settled(shown, waiting);
        if waiting > 0 {
            let newest = live.pop().unwrap();
            notifications.client.close(newest).unwrap();
            assert_eq!(notifications.next_signal(), Signal::Closed(newest, 3));
            notifications.settled(live.len(), 0);
            break;
        }
    }
    std::thread::sleep(QUIET);

    let mark = notifications.daemon().watch().lines().len();
    let one_more = notifications.client.simple(0, "One more", "", 0);
    notifications.settled(live.len(), 1);
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        1,
        "a popup into a full column:\n{}",
        notifications.daemon().watch().tail()
    );

    let mark = notifications.daemon().watch().lines().len();
    notifications.client.close(one_more).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(one_more, 3));
    notifications.settled(live.len(), 0);
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        1,
        "a close bringing a waiting one back:\n{}",
        notifications.daemon().watch().tail()
    );
}
