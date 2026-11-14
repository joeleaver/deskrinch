//! The launcher's root: the look applied, the window following
//! [`Visibility`], and the model ([`Model`]) driven by what the user types
//! and presses.
//!
//! - The query is the field's text; the results follow it and the
//!   installed applications.
//! - Up and Down move the selection; Enter asks for an activation token for
//!   the launcher's window (from the key press) and, in its answer, starts
//!   the selected application with it and hides; Escape hides; so does the
//!   window losing the keyboard while shown.
//! - Hiding clears the model, so the next show starts with an empty query
//!   and the frequent applications as its results.
//! - The frequent applications follow the record of starts, the installed
//!   applications and `[general]` `remember-starts` (which the record's
//!   thread follows too). A change to them reaches the model at once while
//!   the launcher is hidden, so that its rows are built before the next
//!   show, and only at the next hide while it is shown. The first, made
//!   with the window, is the model's from the start.
//! - While hidden, the window is prepared for its next show (rinch styles,
//!   lays out and paints it without presenting) at start and whenever what
//!   that show draws changes: see [`prepare_while_hidden`].

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_component::LiveSettings;
use deskrinch_desktop_entries::{DesktopEntry, Environment};
use deskrinch_icons::Index;
use deskrinch_launcher::{Action, Drawn, Input, Model, Preparation, Visibility, frequent};
use deskrinch_settings::general::GeneralSettings;
use deskrinch_start_record::Record;
use deskrinch_theme::{Scheme, Theme};
use rinch::prelude::*;
use rinch::reactive::Effect;
use rinch::windows::{hide_current_window, prepare_current_window, show_current_window};

use crate::start::start;
use crate::starts::{Held, Starts};
use crate::view;

/// What the launcher runs with.
pub(crate) struct Parts {
    pub(crate) theme: Theme,
    /// `[general]`, for the terminal applications run in.
    pub(crate) general: LiveSettings<GeneralSettings>,
    pub(crate) visibility: Arc<Visibility>,
    /// Whether the window was made shown.
    pub(crate) made_shown: bool,
    pub(crate) installed: Installed,
    pub(crate) icons: Icons,
    pub(crate) env: Environment,
    /// Where starts are recorded, and `remember-starts` is followed.
    pub(crate) starts: Starts,
    /// The record of starts, for the frequent applications.
    pub(crate) record: Held,
}

/// The launcher, following its parts until it closes.
#[component]
pub(crate) fn launcher(parts: Parts) -> NodeHandle {
    let Parts {
        theme,
        general,
        visibility,
        made_shown,
        installed,
        icons,
        env,
        starts,
        record,
    } = parts;
    let applied = theme.apply();
    let general = general.into_signal();
    let env = Rc::new(env);
    let installed = installed.into_signal();
    // The ones shown in menus: unchanged by a change to the others.
    let entries = Memo::new(move || installed.with(|all| all.shown()));
    let model = Signal::new(Model::new());
    let shown = visibility.signal();
    let record = record.into_signal();

    icons.follow_theme(applied.icon_theme);
    let index = icons.signal();

    // The applications changed: search them again for the same query.
    {
        let env = Rc::clone(&env);
        Effect::new(move || {
            let entries = entries.get();
            untracked(|| model.update(|m| m.refresh(entries.iter(), &env.locales)));
        });
    }

    follow_frequent(&Frequent {
        starts: &starts,
        record,
        general,
        entries,
        env: &env,
        model,
        shown,
    });

    let on_query = {
        let env = Rc::clone(&env);
        move |query: String| {
            let entries = entries.get();
            model.update(|m| m.set_query(&query, entries.iter(), &env.locales));
        }
    };
    let starter = Starter {
        visibility: Arc::clone(&visibility),
        entries,
        general,
        env,
        starts,
    };
    let on_key = {
        let starter = starter.clone();
        move |key: &str| -> bool {
            let input = match key {
                "ArrowUp" => Input::Up,
                "ArrowDown" => Input::Down,
                "Enter" => Input::Enter,
                "Escape" => Input::Escape,
                _ => return false,
            };
            starter.act(self::input(model, input));
            true
        }
    };
    let on_pointer: Rc<dyn Fn(Input)> = {
        let starter = starter.clone();
        Rc::new(move |pointer: Input| starter.act(input(model, pointer)))
    };
    let (content, field) = view::view(__scope, model, index, on_query, on_key, on_pointer);

    // The window follows `Visibility`; showing puts the keyboard in the
    // field (a window made shown too), hiding empties the model. On the
    // desktop `focus()` posts a request the runtime applies on its next
    // turn, once the field is mounted and the window is up.
    {
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
                untracked(|| model.update(Model::clear));
            }
        });
    }

    hide_when_left(starter, model);
    prepare_while_hidden(model, index, applied.scheme, shown);
    content
}

/// Has rinch prepare the hidden window for its next show whenever what that
/// show draws changes while the launcher is hidden, the first time
/// included ([`Preparation`]): the frequent applications (which the
/// installed applications, the record of starts and `[general]` make), the
/// icon theme's index, and the look. The request is queued and runs on the
/// event loop's next turn, after this turn's changes are in the document.
fn prepare_while_hidden(
    model: Signal<Model>,
    index: Signal<Option<Arc<Index>>>,
    look: Memo<Scheme>,
    shown: Signal<bool>,
) {
    let drawn = Memo::new(move || Drawn {
        rows: model.with(|m| m.frequent().to_vec()),
        icons: index.get(),
        look: look.get(),
    });
    let mut preparation = Preparation::default();
    Effect::new(move || {
        let drawn = drawn.get();
        let changes = preparation.wants(drawn, untracked(|| shown.get()));
        if !changes.is_empty() {
            tracing::info!(
                "preparing the hidden window for its next show: {} changed",
                changes.join(", ")
            );
            prepare_current_window();
        }
    });
}

/// Leaving the launcher (another window took the keyboard, or an
/// application it started did) hides it; it never takes the keyboard back.
fn hide_when_left(starter: Starter, model: Signal<Model>) {
    let mut had_keyboard = false;
    Effect::new(move || {
        let focused = window_focused();
        if had_keyboard && !focused && starter.visibility.is_shown() {
            untracked(|| starter.act(input(model, Input::KeyboardLost)));
        }
        had_keyboard = focused;
    });
}

/// What the frequent applications follow, and the model they go to.
struct Frequent<'a> {
    starts: &'a Starts,
    record: Signal<Arc<Record>>,
    general: Signal<GeneralSettings>,
    entries: Memo<Arc<Vec<DesktopEntry>>>,
    env: &'a Rc<Environment>,
    model: Signal<Model>,
    /// Whether the window is shown.
    shown: Signal<bool>,
}

/// Follows the frequent applications into the model: the record's thread
/// follows `remember-starts` (it started with the value at start-up, and
/// passes over one it has), and off, nothing is shown from the record at
/// once, whatever the thread has yet to do. The rows are built into the
/// model while it is hidden, ready for the next show, and never in place
/// of what is shown.
fn follow_frequent(parts: &Frequent<'_>) {
    let &Frequent {
        starts,
        record,
        general,
        entries,
        env,
        model,
        shown,
    } = parts;
    let remember = Memo::new(move || general.with(|g| g.remember_starts));
    let starts = starts.clone();
    Effect::new(move || starts.remember(remember.get()));
    let env = Rc::clone(env);
    let frequent = Memo::new(move || {
        if !remember.get() {
            return Vec::new();
        }
        let entries = entries.get();
        record.with(|record| frequent(record, &entries, &env.locales))
    });
    let mut first = true;
    Effect::new(move || {
        let rows = frequent.get();
        let hidden = !untracked(|| shown.get());
        let apply = std::mem::take(&mut first) || hidden;
        untracked(|| {
            model.update(|m| {
                m.set_frequent(rows);
                if apply {
                    m.clear();
                }
            });
        });
    });
}

/// Gives the model `input`; answers what to do.
fn input(model: Signal<Model>, input: Input) -> Action {
    let mut action = Action::Stay;
    model.update(|m| action = m.input(input));
    action
}

/// What doing what the model answers needs.
#[derive(Clone)]
struct Starter {
    visibility: Arc<Visibility>,
    entries: Memo<Arc<Vec<DesktopEntry>>>,
    /// `[general]`: the terminal applications run in.
    general: Signal<GeneralSettings>,
    env: Rc<Environment>,
    /// Where a start is recorded.
    starts: Starts,
}

impl Starter {
    /// Does what the model answered.
    fn act(&self, action: Action) {
        match action {
            Action::Stay => {}
            Action::Hide => self.visibility.hide(),
            Action::Launch(id) => {
                let Some(entry) = self.entries.get().iter().find(|e| e.id == id).cloned() else {
                    return;
                };
                let setting = self.general.with(|g| g.terminal.clone());
                let visibility = Arc::clone(&self.visibility);
                let env = Rc::clone(&self.env);
                let starts = self.starts.clone();
                // From the key press: the token names it. The launcher hides
                // in the answer, after the start: hidden first, its surface
                // would be gone and the compositor need not answer.
                request_activation_token(move |token| {
                    start(entry, token, &env, setting, starts);
                    visibility.hide();
                });
            }
        }
    }
}
