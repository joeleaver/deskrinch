//! What a change costs in frames: a notification shown, replaced or closed,
//! the waiting set changing with it included, is drawn in exactly one
//! presented frame, and nothing is drawn while nothing
//! changes.
//!
//! A frame of rinch's cached pixels draws nothing and is not counted
//! ([`deskrinch_test_component::drew`]): the software renderer skips its
//! present, and the GPU renderer presents the same pixels again after a
//! change, which this daemon does nothing to ask for.
//!
//! Each change's frame is waited for as an event; what may not come is
//! watched for after it, over the windows below.

use std::time::Duration;

use deskrinch_test_component::drew;
use deskrinch_test_notify::Signal;
use deskrinch_test_wait::comes_true;

use crate::panel::{PANEL_LEAST, Panel};
use crate::support::{Notifications, PADDING, close_button};

/// How long the daemon is watched for a second frame, once the one a change
/// must draw has come: a frame asked for comes within one or two of the
/// compositor's refreshes (16 ms each at 60 Hz), so a second frame of the
/// same change would come many times over inside it, loaded or not.
const QUIET: Duration = Duration::from_millis(500);

/// How long the daemon is watched for a frame while nothing changes: longer
/// than a second, the period a timer that redraws (a blinking caret, a
/// clock) would most likely have.
const IDLE: Duration = Duration::from_millis(1200);

/// How long a pointer resting on a button is watched for a frame, from when
/// the frames its coming drew have come: as long as a tooltip's delay, which
/// rinch's own was (rinch issue 2552), and more.
const RESTING: Duration = Duration::from_secs(2);

/// The frames that drew something in `lines` from line `from` on.
fn presented(lines: &[String], from: usize) -> usize {
    lines
        .get(from..)
        .unwrap_or_default()
        .iter()
        .filter(|line| drew(line))
        .count()
}

/// Waits for the frame a change begun at `mark` draws, then watches
/// [`QUIET`] more; how many frames drew since `mark`.
fn frames_of_a_change(notifications: &Notifications, mark: usize) -> usize {
    let lines = || notifications.daemon().watch().lines();
    // The column's window is not the app's main one (a windowless app has
    // none), so its frames are counted from the lines.
    assert!(
        comes_true(|| presented(&lines(), mark) >= 1),
        "no frame drew:\n{}",
        notifications.daemon().watch().tail()
    );
    std::thread::sleep(QUIET);
    presented(&lines(), mark)
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_change_is_one_frame_and_nothing_changing_is_none() {
    let mut notifications = Notifications::start();
    let id = notifications.client.simple(0, "Frames", "counted", 0);
    notifications.column("the popup", |c| c.popups.len() == 1);

    let mark = notifications.daemon().watch().lines().len();
    std::thread::sleep(IDLE);
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
    assert_eq!(
        frames_of_a_change(&notifications, mark),
        1,
        "a replacement:\n{}",
        notifications.daemon().watch().tail()
    );

    let mark = notifications.daemon().watch().lines().len();
    let second = notifications.client.simple(0, "Another", "below", 0);
    assert_eq!(
        frames_of_a_change(&notifications, mark),
        1,
        "a second popup:\n{}",
        notifications.daemon().watch().tail()
    );
    assert_eq!(notifications.fit_drawn(2), (2, 0));

    let mark = notifications.daemon().watch().lines().len();
    notifications.client.close(second).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(second, 3));
    assert_eq!(
        frames_of_a_change(&notifications, mark),
        1,
        "a popup closed:\n{}",
        notifications.daemon().watch().tail()
    );
    notifications.column("one popup", |c| c.popups.len() == 1);
}

/// The waiting set changing costs a frame like any other change: a popup
/// into a full column (some wait, the count line appears), and a close
/// that brings the waiting back.
///
/// How many wait is the daemon's, read once the screen agrees: the count
/// line can take the room of the last shown popup too. That count, for a
/// column of a known geometry, is the harness's to check
/// (`view::tests::the_count_line_s_room_moves_the_cut`, in the daemon's
/// own tests); here the screen is held to the daemon's word, and the frames
/// counted.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_waiting_set_changing_is_one_frame() {
    let mut notifications = Notifications::start();
    let (live, _) = notifications.full_column();
    notifications.quiet();

    let mark = notifications.daemon().watch().lines().len();
    let one_more = notifications.client.simple(0, "One more", "", 0);
    let (_, fit) = notifications.settled_on("some waiting", |(_, waiting)| waiting > 0);
    let (shown, waiting) = fit;
    assert_eq!(shown + waiting, live.len() + 1, "{fit:?}");
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
    assert_eq!(
        frames_of_a_change(&notifications, mark),
        1,
        "a close bringing a waiting one back:\n{}",
        notifications.daemon().watch().tail()
    );
    assert_eq!(notifications.fit_drawn(live.len()), (live.len(), 0));
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
    let image = &column.image;
    let (x, y) = close_button(image, &column, column.popups[0], notifications.scale);
    notifications.point(&column, x, y);
    notifications.quiet();
    let mark = notifications.daemon().watch().lines().len();
    std::thread::sleep(RESTING);
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
    let (live, column) = notifications.full_column();
    let scale = notifications.scale;
    let (_, height) = column.size;
    let (_, last) = *column.popups.last().expect("a shown popup");
    // The room left under the last popup, in logical pixels.
    let spare = (height - PADDING * scale - (last + 1)) / scale;
    let (panel, base) = if spare >= PANEL_LEAST + LEFT {
        let base = spare - LEFT;
        let panel = Panel::along_the_top(base);
        notifications.settled(live.len(), 0);
        notifications.quiet();
        panel.set_height(base + MORE);
        (panel, Some(base))
    } else {
        // Few pixels left already: a panel of its least height, or a few
        // more, takes them all and more.
        (Panel::along_the_top((spare + MORE).max(PANEL_LEAST)), None)
    };
    notifications.settled(live.len() - 1, 1);
    notifications.quiet();

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
