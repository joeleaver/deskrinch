//! The has-keyboard line ([`HAS_KEYBOARD`]) as the wiring writes it, from
//! the query field's own `focus` event, once per show; and where a key goes
//! once a press elsewhere in the launcher has taken the focus from the
//! field. In rinch's test harness, which gives the window the keyboard at
//! its mount and with each show, and takes it with each hide, as the
//! compositor does.

use deskrinch_launcher::HAS_KEYBOARD;
use rinch::prelude::*;
use tracing::subscriber::DefaultGuard;

use super::tests::{Log, Mount, Wired, wired, wired_as};

/// The log, kept for the rest of the test.
fn logged() -> (Log, DefaultGuard) {
    let log = Log::default();
    let guard = tracing::subscriber::set_default(log.clone());
    (log, guard)
}

/// How many times the line was said.
fn said(log: &Log) -> usize {
    log.lines()
        .iter()
        .filter(|line| line.as_str() == HAS_KEYBOARD)
        .count()
}

/// Ends the show and starts another: the launcher hidden (its window, and
/// so its keyboard, taken), then shown again.
fn hide_and_show(wired: &mut Wired) {
    wired.visibility.hide();
    wired.app.settle();
    wired.visibility.show();
    wired.app.settle();
}

#[test]
fn started_shown_it_says_it_has_the_keyboard_once() {
    let (log, _guard) = logged();
    let mut wired = wired(true);
    wired.app.settle();
    assert_eq!(said(&log), 1, "{:?}", log.lines());
}

#[test]
fn each_show_says_it_once() {
    let (log, _guard) = logged();
    let mut wired = wired(true);
    wired.app.settle();
    hide_and_show(&mut wired);
    assert_eq!(said(&log), 2, "{:?}", log.lines());
    hide_and_show(&mut wired);
    assert_eq!(said(&log), 3, "{:?}", log.lines());
}

#[test]
fn the_field_taking_the_focus_back_within_a_show_says_nothing_more() {
    let (log, _guard) = logged();
    let mut wired = wired_as(Mount {
        shown: true,
        made_shown: true,
        other: true,
    });
    wired.app.settle();
    wired.app.focus(".other");
    wired.app.focus(".launcher-query");
    wired.app.settle();
    assert_eq!(said(&log), 1, "{:?}", log.lines());
}

#[test]
fn nothing_is_said_before_the_field_has_the_focus() {
    let (log, _guard) = logged();
    // Hidden, with the window's keyboard (the harness gives it at the
    // mount) and the field not focused.
    let mut wired = wired(false);
    wired.app.settle();
    assert_eq!(said(&log), 0, "{:?}", log.lines());
    // The show focuses the field: now it has the keyboard.
    wired.visibility.show();
    wired.app.settle();
    assert_eq!(said(&log), 1, "{:?}", log.lines());
}

#[test]
fn nothing_is_said_while_another_element_has_the_focus() {
    let (log, _guard) = logged();
    // The window has the keyboard (the harness gives it at the mount); the
    // field is not focused, and another element takes the focus.
    let mut wired = wired_as(Mount {
        shown: false,
        made_shown: false,
        other: true,
    });
    wired.app.focus(".other");
    wired.app.settle();
    assert_eq!(said(&log), 0, "{:?}", log.lines());
    // The field taking it says so, once.
    wired.app.focus(".launcher-query");
    wired.app.settle();
    assert_eq!(said(&log), 1, "{:?}", log.lines());
}

/// A press off the field, `press`, made after "te" is typed into the shown
/// launcher, then "r" typed: whether the field still has the focus after
/// the press, and the query at the end.
fn key_after(press: impl FnOnce(&mut Wired)) -> (bool, String) {
    let mut wired = wired(true);
    wired.app.type_text("te");
    press(&mut wired);
    let field = wired.app.handle(".launcher-query").node_id();
    let focused = wired.app.focused().map(|node| node.node_id()) == Some(field);
    wired.app.type_text("r");
    let query = wired.model.with(|m| m.query().to_owned());
    (focused, query)
}

/// A press on the launcher's padding away from the field (its bottom right
/// corner) takes the focus from the field, and a key typed then is lost:
/// the query stays "te". What it is today, not what it should be.
#[test]
fn a_press_on_the_padding_takes_the_focus_and_the_next_key_is_lost() {
    let (focused, query) = key_after(|wired| {
        let launcher = wired.app.bounds(".launcher");
        wired.app.click(
            launcher.x + launcher.width - 2.0,
            launcher.y + launcher.height - 2.0,
        );
    });
    assert_eq!((focused, query.as_str()), (false, "te"));
}

/// The same for a press on a result released off it (no click, so no
/// start): the field loses the focus and the next key is lost.
#[test]
fn a_press_on_a_row_released_off_it_takes_the_focus_and_the_next_key_is_lost() {
    let (focused, query) = key_after(|wired| {
        let rows = wired.app.query_all(".launcher-row");
        let launcher = wired.app.bounds(".launcher");
        wired.app.press(&rows[0], MouseButton::Left);
        wired.app.mouse_up(
            launcher.x + launcher.width - 2.0,
            launcher.y + launcher.height - 2.0,
            MouseButton::Left,
        );
    });
    assert_eq!((focused, query.as_str()), (false, "te"));
}
