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
//! - Hiding clears the model, so the next show starts empty.

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_component::LiveSettings;
use deskrinch_desktop_entries::{DesktopEntry, Environment};
use deskrinch_icons::SearchPath;
use deskrinch_launcher::{Action, Input, Model, Visibility};
use deskrinch_settings::general::GeneralSettings;
use deskrinch_theme::{Theme, resolve_icon_theme};
use rinch::prelude::*;
use rinch::reactive::Effect;
use rinch::windows::{hide_current_window, show_current_window};

use crate::apps::Apps;
use crate::icons::Icons;
use crate::start::start;
use crate::view;

/// What the launcher runs with.
pub(crate) struct Parts {
    pub(crate) theme: Theme,
    /// `[general]`, for the terminal applications run in.
    pub(crate) general: LiveSettings<GeneralSettings>,
    pub(crate) visibility: Arc<Visibility>,
    /// Whether the window was made shown.
    pub(crate) made_shown: bool,
    pub(crate) apps: Apps,
    pub(crate) icons: Icons,
    pub(crate) search_path: SearchPath,
    pub(crate) env: Environment,
}

/// The launcher, following its parts until it closes.
#[component]
pub(crate) fn launcher(parts: Parts) -> NodeHandle {
    let Parts {
        theme,
        general,
        visibility,
        made_shown,
        apps,
        icons,
        search_path,
        env,
    } = parts;
    let applied = theme.apply();
    let general = general.into_signal();
    let env = Rc::new(env);
    let entries = apps.into_signal();
    let model = Signal::new(Model::new());
    let shown = visibility.signal();

    follow_icon_theme(applied.icon_theme, &icons, search_path);
    let index = icons.signal();

    // The applications changed: search them again for the same query.
    {
        let env = Rc::clone(&env);
        Effect::new(move || {
            let entries = entries.get();
            untracked(|| model.update(|m| m.refresh(entries.iter(), &env.locales)));
        });
    }

    let on_query = {
        let env = Rc::clone(&env);
        move |query: String| {
            let entries = entries.get();
            model.update(|m| m.set_query(&query, entries.iter(), &env.locales));
        }
    };
    let on_key = {
        let visibility = Arc::clone(&visibility);
        let env = Rc::clone(&env);
        move |key: &str| -> bool {
            let input = match key {
                "ArrowUp" => Input::Up,
                "ArrowDown" => Input::Down,
                "Enter" => Input::Enter,
                "Escape" => Input::Escape,
                _ => return false,
            };
            act(
                self::input(model, input),
                &visibility,
                entries,
                general,
                &env,
            );
            true
        }
    };
    let on_pointer: Rc<dyn Fn(Input)> = {
        let visibility = Arc::clone(&visibility);
        let env = Rc::clone(&env);
        Rc::new(move |pointer: Input| {
            act(input(model, pointer), &visibility, entries, general, &env);
        })
    };
    let (content, field) = view::view(__scope, model, index, on_query, on_key, on_pointer);

    // The window follows `Visibility`; showing puts the keyboard in the
    // field, hiding empties the model.
    {
        let mut applied_shown = made_shown;
        Effect::new(move || {
            let wanted = shown.get();
            if wanted == applied_shown {
                return;
            }
            applied_shown = wanted;
            if wanted {
                show_current_window();
                let field = field.clone();
                set_timeout(0, move || field.focus());
            } else {
                hide_current_window();
                untracked(|| model.update(Model::clear));
            }
        });
    }

    // Leaving the launcher (another window took the keyboard, or an
    // application it started did) hides it; it never takes the keyboard
    // back.
    {
        let mut had_keyboard = false;
        Effect::new(move || {
            let focused = window_focused();
            if had_keyboard && !focused && visibility.is_shown() {
                untracked(|| {
                    act(
                        input(model, Input::KeyboardLost),
                        &visibility,
                        entries,
                        general,
                        &env,
                    );
                });
            }
            had_keyboard = focused;
        });
    }
    content
}

/// Gives the model `input`; answers what to do.
fn input(model: Signal<Model>, input: Input) -> Action {
    let mut action = Action::Stay;
    model.update(|m| action = m.input(input));
    action
}

/// Does what the model answered.
fn act(
    action: Action,
    visibility: &Arc<Visibility>,
    entries: Signal<Arc<Vec<DesktopEntry>>>,
    general: Signal<GeneralSettings>,
    env: &Rc<Environment>,
) {
    match action {
        Action::Stay => {}
        Action::Hide => visibility.hide(),
        Action::Launch(id) => {
            let Some(entry) = entries.with(|all| all.iter().find(|e| e.id == id).cloned()) else {
                return;
            };
            let setting = general.with(|g| g.terminal.clone());
            let visibility = Arc::clone(visibility);
            let env = Rc::clone(env);
            // From the key press: the token names it. The launcher hides in
            // the answer, after the start: hidden first, its surface would
            // be gone and the compositor need not answer.
            request_activation_token(move |token| {
                start(entry, token, &env, setting);
                visibility.hide();
            });
        }
    }
}

/// Resolves the icon theme against what is installed whenever its inputs
/// change, and has `icons` follow it.
fn follow_icon_theme(
    choice: Memo<deskrinch_theme::IconThemeChoice>,
    icons: &Icons,
    search_path: SearchPath,
) {
    let theme = Memo::new(move || {
        let choice = choice.get();
        let resolved = resolve_icon_theme(&choice.setting, &choice.portal, |name| {
            search_path.has_theme(name)
        });
        for missing in &resolved.not_installed {
            tracing::warn!("the icon theme {missing:?} is not installed");
        }
        resolved.name
    });
    let follow = icons.follower();
    Effect::new(move || follow(theme.get()));
}
