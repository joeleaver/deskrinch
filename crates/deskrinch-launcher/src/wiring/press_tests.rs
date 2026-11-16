//! A press anywhere in the shown launcher that is not on the query field
//! leaves the keyboard in the field, its caret where it was: each case
//! types "te", presses (or keys), types "r", and reads the query. A press
//! on the field still places the caret, and a click on a result still
//! starts it. None of these changes the launcher's visibility.

use deskrinch_launcher::Action;
use rinch::prelude::*;

use super::tests::{Wired, wired};

/// The shown launcher with "te" typed, `between` done, then "r" typed:
/// whether the field has the focus at the end, and the query.
fn typed_around(between: impl FnOnce(&mut Wired)) -> (bool, String) {
    let mut wired = wired(true);
    wired.app.type_text("te");
    between(&mut wired);
    wired.app.type_text("r");
    let field = wired.app.handle(".launcher-query").node_id();
    let focused = wired.app.focused().map(|node| node.node_id()) == Some(field);
    let query = wired.model.with(|m| m.query().to_owned());
    (focused, query)
}

#[test]
fn a_press_on_the_padding_far_from_the_field_keeps_the_keyboard_in_it() {
    let after = typed_around(|wired| {
        let launcher = wired.app.bounds(".launcher");
        wired.app.click(
            launcher.x + launcher.width - 2.0,
            launcher.y + launcher.height - 2.0,
        );
    });
    assert_eq!(after, (true, "ter".to_owned()));
}

#[test]
fn a_press_on_the_padding_beside_the_field_keeps_the_keyboard_and_the_caret() {
    let after = typed_around(|wired| {
        let launcher = wired.app.bounds(".launcher");
        let field = wired.app.bounds(".launcher-query");
        let (x, y) = (launcher.x + 2.0, launcher.y + 2.0);
        assert!(
            x < field.x && y < field.y,
            "the point is the padding, not the field: {field:?}"
        );
        wired.app.click(x, y);
    });
    assert_eq!(after, (true, "ter".to_owned()));
}

#[test]
fn a_press_on_a_result_released_off_it_keeps_the_keyboard_in_the_field() {
    let after = typed_around(|wired| {
        let rows = wired.app.query_all(".launcher-row");
        let launcher = wired.app.bounds(".launcher");
        wired.app.press(&rows[0], MouseButton::Left);
        wired.app.mouse_up(
            launcher.x + launcher.width - 2.0,
            launcher.y + launcher.height - 2.0,
            MouseButton::Left,
        );
    });
    assert_eq!(after, (true, "ter".to_owned()));
}

#[test]
fn tab_keeps_the_keyboard_in_the_field() {
    let after = typed_around(|wired| wired.app.press_key("Tab"));
    assert_eq!(after, (true, "ter".to_owned()));
}

#[test]
fn a_press_on_the_field_places_its_caret() {
    // At the field's left edge, inside its border: before its text.
    let after = typed_around(|wired| {
        let field = wired.app.bounds(".launcher-query");
        wired.app.click(field.x + 2.0, field.y + field.height / 2.0);
    });
    assert_eq!(after, (true, "rte".to_owned()));
}

#[test]
fn a_click_on_a_result_starts_it() {
    let mut wired = wired(true);
    wired.app.type_text("term");
    let rows = wired.app.query_all(".launcher-row");
    let first = wired.model.with(|m| m.rows()[m.visible()][0].id.clone());
    wired.app.click_on(&rows[0]);
    let done: Vec<Action> = wired
        .acts
        .borrow()
        .iter()
        .filter(|action| **action != Action::Stay)
        .cloned()
        .collect();
    assert_eq!(done, [Action::Launch(first)]);
}
