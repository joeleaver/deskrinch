//! What the user does with the launcher, made into its model's input, and
//! the window following whether the launcher is shown: the part of the
//! root ([`crate::root`]) that is the launcher's own behaviour, apart from
//! the world it reads (the look, the installed applications, the record of
//! starts) and what it acts on (starting an application).
//!
//! - The query is the field's text; Up and Down move the selection, Enter
//!   and Escape go to the model; presses and releases go to it as the view
//!   makes them ([`view`]). What the model answers is handed to `act`.
//! - The window follows [`Visibility`]: showing puts the keyboard in the
//!   field (a window made shown too); hiding says how long the query was
//!   and empties the model, so the next show starts with an empty query.
//! - The field gaining the keyboard while the launcher is shown, once per
//!   show, it says [`HAS_KEYBOARD`] ([`say_has_keyboard`]).
//! - Leaving the launcher hides it ([`Input::KeyboardLost`],
//!   [`hide_when_left`]).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use deskrinch_desktop_entries::CatalogEntry;
use deskrinch_icons::Index;
use deskrinch_launcher::{Action, HAS_KEYBOARD, Input, Model, Visibility, query_at_hide};
use rinch::prelude::*;
use rinch::reactive::Effect;
use rinch::windows::{hide_current_window, show_current_window};

use crate::view;

/// What the wiring reads and acts through.
pub(crate) struct Wiring {
    pub(crate) model: Signal<Model>,
    /// The applications shown in menus, which a query searches.
    pub(crate) entries: Memo<Arc<Vec<Arc<CatalogEntry>>>>,
    /// The icon theme's index, for the results' icons.
    pub(crate) index: Signal<Option<Arc<Index>>>,
    pub(crate) visibility: Arc<Visibility>,
    /// The visibility's signal on this thread ([`Visibility::signal`],
    /// which is taken once).
    pub(crate) shown: Signal<bool>,
    /// Whether the window was made shown.
    pub(crate) made_shown: bool,
    /// Does what the model answers.
    pub(crate) act: Rc<dyn Fn(Action)>,
}

/// The launcher's content, wired: the user's input to the model and its
/// answers to `act`, the window following the visibility.
pub(crate) fn wire(__scope: &mut RenderScope, wiring: Wiring) -> NodeHandle {
    let Wiring {
        model,
        entries,
        index,
        visibility,
        shown,
        made_shown,
        act,
    } = wiring;
    let on_query = move |query: String| {
        let entries = entries.get();
        model.update(|m| m.set_query(&query, entries.iter().map(|e| &e.entry)));
    };
    let on_key = {
        let act = Rc::clone(&act);
        move |key: &str| -> bool {
            let Some(input) = key_input(key) else {
                return false;
            };
            act(self::input(model, input));
            true
        }
    };
    let on_pointer: Rc<dyn Fn(Input)> = {
        let act = Rc::clone(&act);
        Rc::new(move |pointer: Input| act(input(model, pointer)))
    };
    let said = Rc::new(Cell::new(false));
    let on_focus = say_has_keyboard(Rc::clone(&said));
    let (content, field) = view::view(
        __scope, model, index, on_query, on_key, on_pointer, on_focus,
    );
    follow_visibility(shown, made_shown, field, model, said);
    hide_when_left(visibility, model, act);
    content
}

/// What the field's `focus` event does: says [`HAS_KEYBOARD`] the first
/// time in a show that the field gains the keyboard. The line is once a
/// show whatever moves the focus: nothing in the launcher takes it from
/// the field today (a press off the field keeps it there, `data-nofocus`
/// in [`view`], and Tab has no other stop), but each return of the focus
/// would be another `focus` event, so `said` keeps the line to one a show,
/// and the show's end clears it ([`follow_visibility`]). The field is
/// focused only by a show, so its `focus` event comes only while the
/// launcher is shown.
fn say_has_keyboard(said: Rc<Cell<bool>>) -> impl Fn() + 'static {
    move || {
        if !said.replace(true) {
            tracing::info!("{HAS_KEYBOARD}");
        }
    }
}

/// The model's input for the key `key` names, if the launcher takes it.
fn key_input(key: &str) -> Option<Input> {
    Some(match key {
        "ArrowUp" => Input::Up,
        "ArrowDown" => Input::Down,
        "Enter" => Input::Enter,
        "Escape" => Input::Escape,
        _ => return None,
    })
}

/// The window following `shown`: showing puts the keyboard in `field` (a
/// window made shown too), hiding says how long the query was, empties the
/// model, and ends the show for [`say_has_keyboard`] (`said`). On the
/// desktop `focus()` posts a request rinch applies before the window's
/// first frame (made shown) or its next one, and before the keyboard
/// reaches the window.
fn follow_visibility(
    shown: Signal<bool>,
    made_shown: bool,
    field: NodeHandle,
    model: Signal<Model>,
    said: Rc<Cell<bool>>,
) {
    if made_shown {
        field.focus();
    }
    let mut applied_shown = made_shown;
    Effect::new(move || {
        let wanted = shown.get();
        if wanted == applied_shown {
            return;
        }
        applied_shown = wanted;
        if wanted {
            show_current_window();
            field.focus();
        } else {
            hide_current_window();
            said.set(false);
            untracked(|| {
                let chars = model.with(|m| m.query().chars().count());
                tracing::info!("{}", query_at_hide(chars));
                model.update(Model::clear);
            });
        }
    });
}

/// Leaving the launcher (another window took the keyboard, or an
/// application it started did) hides it; it never takes the keyboard back.
/// It follows the window's keyboard, not the field's: where the focus is
/// inside the launcher is not leaving it.
fn hide_when_left(visibility: Arc<Visibility>, model: Signal<Model>, act: Rc<dyn Fn(Action)>) {
    let mut had_keyboard = false;
    Effect::new(move || {
        let focused = window_focused();
        if had_keyboard && !focused && visibility.is_shown() {
            untracked(|| act(input(model, Input::KeyboardLost)));
        }
        had_keyboard = focused;
    });
}

/// Gives the model `input`; answers what to do.
fn input(model: Signal<Model>, input: Input) -> Action {
    let mut action = Action::Stay;
    model.update(|m| action = m.input(input));
    action
}

#[cfg(test)]
mod keyboard_tests;
#[cfg(test)]
mod press_tests;
#[cfg(test)]
mod tests;
