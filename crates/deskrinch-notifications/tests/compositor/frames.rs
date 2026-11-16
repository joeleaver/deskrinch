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

use crate::panel::{PANEL_LEAST, Panel};
use crate::support::{Notifications, PADDING, close_button};

/// How long the daemon is watched for a frame it must not draw: a frame
/// asked for by a change comes within a few of the compositor's refreshes.
const QUIET: Duration = Duration::from_secs(2);

/// Fills the column, with no room to spare for another popup: popups until
/// one waits, then the newest closed. The live ones, oldest first, all
/// shown.
fn full_column(notifications: &mut Notifications) -> Vec<u32> {
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
            return live;
        }
    }
}

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
    let live = full_column(&mut notifications);
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

/// A pointer resting on a button draws nothing: no tooltip, no frame. Its
/// coming may draw (a hover style); once it has come, nothing.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_pointer_resting_on_a_button_draws_nothing() {
    let mut notifications = Notifications::start();
    notifications
        .client
        .simple(0, "Rest", "on its close button", 0);
    let column = notifications.column("the popup", |c| c.popups.len() == 1);
    let image = notifications.picture(&column);
    let (x, y) = close_button(&image, &column, column.popups[0], notifications.scale);
    notifications.point(&column, x, y);
    std::thread::sleep(QUIET);
    let mark = notifications.daemon().watch().lines().len();
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        0,
        "frames while the pointer rested on the close button:\n{}",
        notifications.daemon().watch().tail()
    );
}

/// The room coming back by less than the count line's own room: the
/// waiting popup comes back and the line goes, in one frame (the fit's one
/// judgment beyond the layout, `fit::shown`, which the layout without the
/// line then confirms).
///
/// A full column first, with the room left under it few pixels (fewer
/// than the line's room: its line, some 17 pixels, and its gap, 12): as
/// filled where that is already so, else with the panel along the top
/// taking all but `LEFT` of it. Then the room is taken by a few more
/// pixels, so that the oldest waits, and given back: the panel made taller
/// and then as it was, or, where there was none, started and then ended.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_popup_back_as_the_count_line_goes_is_one_frame() {
    /// The pixels left under the full column, fewer than the line's room.
    const LEFT: u32 = 4;
    /// How much more is taken, so that the oldest waits.
    const MORE: u32 = 8;
    let mut notifications = Notifications::start();
    let live = full_column(&mut notifications);
    let column = notifications.settled(live.len(), 0);
    let scale = notifications.scale;
    let (_, height) = column.size;
    let (_, last) = *column.popups.last().expect("a shown popup");
    // The room left under the last popup, in logical pixels.
    let spare = (height - PADDING * scale - (last + 1)) / scale;
    let (panel, base) = if spare >= PANEL_LEAST + LEFT {
        let base = spare - LEFT;
        let panel = Panel::along_the_top(base);
        notifications.settled(live.len(), 0);
        std::thread::sleep(QUIET);
        panel.set_height(base + MORE);
        (panel, Some(base))
    } else {
        // Few pixels left already: a panel of its least height, or a few
        // more, takes them all and more.
        (Panel::along_the_top((spare + MORE).max(PANEL_LEAST)), None)
    };
    notifications.settled(live.len() - 1, 1);
    std::thread::sleep(QUIET);

    let mark = notifications.daemon().watch().lines().len();
    match base {
        Some(base) => panel.set_height(base),
        None => drop(panel),
    }
    notifications.settled(live.len(), 0);
    std::thread::sleep(QUIET);
    assert_eq!(
        presented(&notifications.daemon().watch().lines(), mark),
        1,
        "a popup back as the count line went:\n{}",
        notifications.daemon().watch().tail()
    );
}
