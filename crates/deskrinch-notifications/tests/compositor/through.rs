//! What the column's window lets through: the pointer, everywhere but over
//! a shown popup. The window spans the output's height and is
//! transparent; it takes the pointer only over the shown popups
//! (`InputRegion::Marked`), so a press on the gaps between them, the
//! column's padding, the count line, the room under them or a hidden
//! popup reaches what is under it: here a window of the test's own on
//! every output ([`Beneath`]), which records what it hears.
//!
//! Each press is checked twice: the window under hears the pointer come to
//! the point, then the click there, its press and its release.

use std::collections::HashMap;

use deskrinch_test_client::PointerEvent;
use deskrinch_test_notify::Signal;

use crate::panel::Panel;
use crate::support::{
    Beneath, Column, Notifications, PADDING, WIDTH, clicked_since, close_button, over_since,
    pressed_since,
};

/// The reason `NotificationClosed` carries for a dismissal.
const DISMISSED: u32 = 2;

/// The tallest panel there is, in logical pixels.
const PANEL: u32 = 256;

/// Presses at `(x, y)`, the screen's pixels on `column`'s output, and
/// asserts that the window under the column hears the pointer come there
/// and the whole click; `what` is what is there, for a failure.
fn reaches(
    notifications: &mut Notifications,
    beneath: &Beneath,
    column: &Column,
    (x, y): (u32, u32),
    what: &str,
) {
    let window = beneath.under(column);
    let mark = window.pointer_events().len();
    notifications.point(column, x, y);
    let at = notifications.logical(x, y);
    let heard = || window.pointer_events().split_off(mark);
    if let Err(error) = window.wait_pointer(&format!("the pointer over {what}"), |events| {
        over_since(events, mark, at)
    }) {
        panic!(
            "{error}: {what} at {at:?} took the pointer; heard {:?}, column {column:?}",
            heard()
        );
    }
    notifications.press();
    if let Err(error) = window.wait_pointer(&format!("a click on {what}"), |events| {
        clicked_since(events, mark)
    }) {
        panic!(
            "{error}: {what} at {at:?} took the press; heard {:?}, column {column:?}",
            heard()
        );
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_gaps_padding_and_strip_below_let_the_pointer_through_and_a_popup_takes_it() {
    let beneath = Beneath::open();
    let mut notifications = Notifications::start();
    notifications.client.simple(0, "Lower", "the older", 0);
    let upper = notifications.client.simple(0, "Upper", "the newer", 0);
    let column = notifications.column("two popups", |c| c.popups.len() == 2);
    let scale = notifications.scale;
    let (width, height) = column.size;
    let middle = column.left.midpoint(column.right);
    let [(upper_top, upper_bottom), (lower_top, lower_bottom)] = column.popups[..] else {
        unreachable!("two popups");
    };
    let beside = column.right + 1 + PADDING * scale / 2;
    assert!(beside < width, "no padding right of the popups: {column:?}");

    let presses = [
        (
            (middle, upper_bottom.midpoint(lower_top)),
            "the gap between the popups",
        ),
        (
            (middle, column.top / 2),
            "the column's padding above the popups",
        ),
        (
            (beside, upper_top.midpoint(upper_bottom)),
            "the column's padding beside a popup",
        ),
        (
            (middle, lower_bottom.midpoint(height)),
            "the strip below the column",
        ),
    ];
    for (point, what) in presses {
        reaches(&mut notifications, &beneath, &column, point, what);
    }

    // A shown popup takes the press, and does what it does: its close
    // button dismisses it. The window under hears the pointer leave, and
    // no press.
    let window = beneath.under(&column);
    let mark = window.pointer_events().len();
    let image = &column.image;
    let (x, y) = close_button(image, &column, column.popups[0], scale);
    notifications.click(&column, x, y);
    assert_eq!(
        notifications.next_signal(),
        Signal::Closed(upper, DISMISSED)
    );
    let heard = window.pointer_events().split_off(mark);
    assert!(
        !pressed_since(&heard, 0) && heard.contains(&PointerEvent::Left),
        "the window under a shown popup heard {heard:?}"
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_count_line_and_the_room_under_the_popups_let_the_pointer_through() {
    let beneath = Beneath::open();
    let mut notifications = Notifications::start();
    // Popups until one waits: the count line shows.
    let (_, (shown, waiting)) = notifications.fill();
    let column = notifications.settled(shown, waiting);
    let (left, top, right, bottom) = column.count_line.expect("the count line");
    let (_, last) = *column.popups.last().expect("a shown popup");
    let middle = column.left.midpoint(column.right);
    let presses = [
        (
            (left.midpoint(right), top.midpoint(bottom)),
            "the count line",
        ),
        (
            (middle, last.midpoint(top)),
            "the room under the shown popups",
        ),
    ];
    for (point, what) in presses {
        reaches(&mut notifications, &beneath, &column, point, what);
    }
}

/// The case the marking was written for: a first popup taller than the
/// room waits, hidden, where it would be; the pointer pressed there
/// reaches what is under it.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_hidden_first_popup_lets_the_pointer_through() {
    // The panel first, presented before a fullscreen window covers it: the
    // tallest there is, along the top, so that the room is as short as it
    // gets.
    let _panel = Panel::along_the_top(PANEL);
    let beneath = Beneath::open();
    let mut notifications = Notifications::start();
    // As tall as a popup gets: six lines of body, and the most actions the
    // service keeps, each a long label that fills its row.
    let actions: Vec<(String, String)> = (1..=16)
        .map(|n| {
            (
                format!("action-{n}"),
                format!(
                    "Action {n}, with a label long enough to fill the popup's width and \
                     wrap onto further lines of its button, as long as the service lets a \
                     label be, so that sixteen of them make a popup taller than any room"
                ),
            )
        })
        .collect();
    let actions: Vec<&str> = actions
        .iter()
        .flat_map(|(key, label)| [key.as_str(), label.as_str()])
        .collect();
    let six = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";
    notifications.send(0, "Tall", six, &actions, &HashMap::new(), 0);
    let column = notifications.settled(0, 1);
    let scale = notifications.scale;
    let (width, _) = column.size;
    let (_, count_top, _, _) = column.count_line.expect("the count line");
    // In the middle of the column's width, halfway down the room, which
    // starts under the panel: inside the hidden popup, which starts at the
    // room's top, inside its padding, and is taller than it.
    let x = width - WIDTH * scale / 2;
    let y = ((PANEL + PADDING) * scale).midpoint(count_top);
    reaches(
        &mut notifications,
        &beneath,
        &column,
        (x, y),
        "the hidden popup",
    );
}
