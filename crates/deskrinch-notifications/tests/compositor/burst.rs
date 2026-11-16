//! Notifications that change the column while the window's size is being
//! asked for: before its first frame, during it and right after, of
//! different heights, with closes and a replacement mixed in, so that the
//! column grows and shrinks while a size request is in flight (rinch's
//! fork holds a request made while another is in flight, and sends it when
//! the first ends).
//!
//! What must hold at the end, on the screen: every live notification's
//! popup drawn whole (its bottom border as drawn as its top), so none is
//! cut off by a window too small; and the surface's size the content's,
//! as rinch reports the surface it painted (`surface_px`).

use deskrinch_test_component::{PerfLine, perf};
use deskrinch_test_notify::Signal;
use deskrinch_test_screen::Image;
use deskrinch_test_wait::PATIENCE;

use crate::support::{Column, Notifications};

/// A body `lines` lines long.
fn body(lines: usize) -> String {
    (1..=lines)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the popup in rows `popup` of `image` is drawn whole: the middle
/// of its last row is its border, as the middle of its first row is, and
/// unlike the popup's own colour just inside it.
fn whole(image: &Image, column: &Column, popup: (u32, u32), scale: u32) -> bool {
    let (first, last) = popup;
    let middle = column.left.midpoint(column.right);
    let top = image.pixel(middle, first);
    let bottom = image.pixel(middle, last);
    let inside = image.pixel(middle, first + 4 * scale);
    top.is_some() && top == bottom && top != inside
}

/// The size of the surface rinch last painted, in pixels, from its frame
/// lines; `None` before any.
fn surface_px(lines: &[String]) -> Option<u64> {
    lines
        .iter()
        .rev()
        .filter(|line| matches!(perf::parse(line), Some(PerfLine::Frame { .. })))
        .find_map(|line| perf::counter(line, "surface_px"))
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn changes_while_a_size_is_asked_for_leave_every_popup_whole() {
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

    // Left: `last` on top, `short` (now six lines) under it.
    let column = notifications.column("two whole popups", |c| c.popups.len() == 2);
    let image = notifications.picture(&column);
    for (at, popup) in column.popups.iter().enumerate() {
        assert!(
            whole(&image, &column, *popup, scale),
            "popup {at} is not drawn whole: {column:?}\nthe daemon's last lines:\n{}",
            notifications.daemon().watch().tail()
        );
    }
    let watch = notifications.daemon().watch();
    let painted = surface_px(&watch.lines()).expect("a frame painted");
    // The column's content: its width, and down to the last popup's bottom
    // and the padding under it.
    let padding = 12 * scale;
    let width = u64::from(380 * scale);
    let height = u64::from(column.bottom + 1 + padding);
    let content = width * height;
    // A pixel row either way, for rounding the logical height.
    let slack = 2 * width;
    assert!(
        painted.abs_diff(content) <= slack,
        "the surface ({painted} px) is not the content's ({width} x {height} = {content} px): \
         {column:?}\nthe daemon's last lines:\n{}",
        watch.tail()
    );
}
