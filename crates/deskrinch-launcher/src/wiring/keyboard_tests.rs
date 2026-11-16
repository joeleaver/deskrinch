//! The has-keyboard line ([`HAS_KEYBOARD`]) as the wiring writes it, from
//! the query field's own `focus` event, once per show. In rinch's test
//! harness, which gives the window the keyboard at its mount and with each
//! show, and takes it with each hide, as the compositor does.

use deskrinch_launcher::HAS_KEYBOARD;
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
