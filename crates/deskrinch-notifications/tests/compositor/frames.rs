//! What a change costs in frames: a notification shown, replaced or closed
//! is drawn in one presented frame, and nothing is drawn while nothing
//! changes.
//!
//! A frame rinch skips presenting (`soft_presents_skipped`), which sway and
//! labwc ask for once after a change, draws nothing and is not counted.

use std::time::Duration;

use deskrinch_test_component::{PerfLine, perf};
use deskrinch_test_notify::Signal;

use crate::support::Notifications;

/// How long the daemon is watched for a frame it must not draw: a frame
/// asked for by a change comes within a few of the compositor's refreshes.
const QUIET: Duration = Duration::from_secs(2);

/// The frames presented in `lines` from line `from` on.
fn presented(lines: &[String], from: usize) -> usize {
    lines
        .get(from..)
        .unwrap_or_default()
        .iter()
        .filter(|line| matches!(perf::parse(line), Some(PerfLine::Frame { .. })))
        .filter(|line| perf::counter(line, "soft_presents_skipped").unwrap_or(0) == 0)
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

    let frames = notifications.daemon().watch().frames();
    let replaced = notifications
        .client
        .simple(id, "Frames", "counted again", 0);
    assert_eq!(replaced, id);
    assert!(
        notifications
            .daemon()
            .watch()
            .wait_frame_after(frames, deskrinch_test_wait::PATIENCE)
    );
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
