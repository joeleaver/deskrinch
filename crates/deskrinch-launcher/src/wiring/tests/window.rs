//! What the wiring asks of the launcher's window, as `TestApp` records and
//! applies it: shown and hidden as [`Visibility`] says, in order, the
//! keyboard in the field on a show, and the keyboard lost while shown
//! hiding the launcher. That a real layer surface is mapped, unmapped and
//! given the keyboard is the compositor tests'.

use deskrinch_launcher::{Action, Visibility};
use rinch::testing::WindowRequest;

use super::{Wired, wired};
use crate::surface;

impl Wired {
    /// Whether the query field has the keyboard within the window.
    fn field_focused(&self) -> bool {
        let field = self.app.handle(".launcher-query");
        self.app
            .focused()
            .is_some_and(|focused| focused.node_id() == field.node_id())
    }

    /// Changes the visibility with `change` and runs the loop until the
    /// window has followed; answers the window requests made meanwhile.
    fn after(&mut self, change: impl FnOnce(&Visibility)) -> Vec<WindowRequest> {
        self.app.take_window_requests();
        change(&self.visibility);
        self.app.settle();
        self.app.take_window_requests()
    }
}

#[test]
fn made_hidden_the_window_is_hidden_without_the_keyboard_and_asks_nothing() {
    let mut wired = wired(false);
    let props = surface::window_props(false);
    assert_eq!(wired.app.size(), (props.width, props.height));
    assert!(!wired.app.is_visible());
    assert!(!wired.app.is_window_focused());
    assert!(!wired.field_focused());
    assert_eq!(wired.app.take_window_requests(), []);
}

#[test]
fn made_shown_the_field_has_the_keyboard_and_nothing_is_asked() {
    let mut wired = wired(true);
    assert!(wired.app.is_visible());
    assert!(wired.app.is_window_focused());
    assert!(wired.field_focused());
    assert_eq!(wired.app.take_window_requests(), []);
}

#[test]
fn a_show_shows_the_window_and_puts_the_keyboard_in_the_field() {
    let mut wired = wired(false);
    assert_eq!(wired.after(Visibility::show), [WindowRequest::Show]);
    assert!(wired.app.is_visible());
    assert!(wired.app.is_window_focused());
    assert!(wired.field_focused());
}

#[test]
fn a_hide_hides_the_window_and_takes_its_keyboard() {
    let mut wired = wired(true);
    assert_eq!(wired.after(Visibility::hide), [WindowRequest::Hide]);
    assert!(!wired.app.is_visible());
    assert!(!wired.app.is_window_focused());
}

#[test]
fn a_toggle_shows_and_a_second_hides() {
    let mut wired = wired(false);
    assert_eq!(
        wired.after(|v| {
            v.toggle();
        }),
        [WindowRequest::Show]
    );
    assert_eq!(
        wired.after(|v| {
            v.toggle();
        }),
        [WindowRequest::Hide]
    );
    assert!(!wired.app.is_visible());
}

#[test]
fn a_show_while_shown_and_a_hide_while_hidden_ask_nothing() {
    let mut shown = wired(true);
    assert_eq!(shown.after(Visibility::show), []);
    assert!(shown.app.is_visible());
    let mut hidden = wired(false);
    assert_eq!(hidden.after(Visibility::hide), []);
    assert!(!hidden.app.is_visible());
}

#[test]
fn the_keyboard_lost_while_shown_hides() {
    let mut wired = wired(true);
    wired.app.window_focus(false);
    assert_eq!(wired.done(), [Action::Hide]);
}

#[test]
fn a_hidden_launcher_neither_gains_nor_loses_the_keyboard() {
    let mut wired = wired(false);
    // A hidden window cannot gain the keyboard, so it has none to lose.
    wired.app.window_focus(true);
    assert!(!wired.app.is_window_focused());
    wired.app.window_focus(false);
    assert_eq!(wired.done(), []);
}

#[test]
fn the_keyboard_lost_again_after_the_hide_does_nothing_more() {
    let mut wired = wired(true);
    wired.app.window_focus(false);
    // What `act` does with the hide (the root hides the launcher).
    assert_eq!(wired.after(Visibility::hide), [WindowRequest::Hide]);
    wired.app.window_focus(false);
    assert_eq!(wired.done(), [Action::Hide]);
}
