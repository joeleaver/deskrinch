//! Changes in quick succession: notifications shown before the window's
//! first frame, during it and right after, of different heights, with
//! closes and a replacement mixed in.
//!
//! It was written for a content-sized window, whose size request could be
//! in flight while the column changed (rinch's fork holds a request made
//! while another is in flight). The column's window is no longer sized by
//! its content: its height is the compositor's and its width fixed, so no
//! size is ever asked for, and this no longer exercises that. It still
//! exercises the layout and the fit through a burst of changes (and it is
//! what found rinch issue 2748, a freed line clamp panicking the next
//! layout): at the end every shown popup is drawn whole, and those shown
//! and those waiting are the live ones.

use deskrinch_test_component::{PerfLine, perf};
use deskrinch_test_notify::Signal;
use deskrinch_test_wait::PATIENCE;

use crate::support::{Notifications, whole};

/// A body `lines` lines long.
fn body(lines: usize) -> String {
    (1..=lines)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn changes_in_quick_succession_leave_every_shown_popup_whole() {
    let mut notifications = Notifications::start();
    let scale = notifications.scale;
    let watch_line = |n: &Notifications, wanted: &dyn Fn(&str) -> bool, what: &str| {
        let watch = n.daemon().watch();
        assert!(
            watch.wait_for_line(PATIENCE, wanted),
            "{what}: its last lines:\n{}",
            watch.tail()
        );
    };

    // Before the window: the first, tall.
    let tall = notifications.client.simple(0, "Tall", &body(5), 0);
    // Once the window is made, before its first frame: a short one.
    watch_line(&notifications, &|l| l.contains("opened:"), "the window");
    let short = notifications.client.simple(0, "Short", "one line", 0);
    // Right after the first frame: a medium one, the tall one closed, and
    // another tall one, in a row.
    watch_line(
        &notifications,
        &|l| matches!(perf::parse(l), Some(PerfLine::Frame { .. })),
        "the first frame",
    );
    let medium = notifications.client.simple(0, "Medium", &body(3), 0);
    notifications.client.close(tall).unwrap();
    notifications.client.simple(0, "Last", &body(4), 0);
    // And at once: the short one made tall, the medium one closed.
    notifications.client.simple(short, "Short", &body(6), 0);
    notifications.client.close(medium).unwrap();
    assert_eq!(notifications.next_signal(), Signal::Closed(tall, 3));
    assert_eq!(notifications.next_signal(), Signal::Closed(medium, 3));

    // Left: `last` and `short` (now six lines); both shown where the room
    // holds them (scale 1), the second waiting where it does not (scale 2,
    // 360 logical pixels high). Two were live before the last change too
    // (the short one and the medium one, before `last`), so the state the
    // burst ends in is waited for as an event: its last change signalled,
    // then the daemon done with it, saying nothing more; its fit then.
    notifications.quiet();
    let (shown, waiting) = notifications.fit_drawn(2);
    let column = notifications.settled(shown, waiting);
    let image = &column.image;
    for (at, popup) in column.popups.iter().enumerate() {
        assert!(
            whole(image, &column, *popup, scale),
            "popup {at} is not drawn whole: {column:?}\nthe daemon said:\n{}",
            notifications.said()
        );
    }
}
