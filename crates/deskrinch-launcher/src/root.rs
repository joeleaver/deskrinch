//! The launcher's root: the world the launcher reads and acts on, around
//! its wiring ([`wiring`]), which makes what the user types and presses
//! into the model's input and has the window follow [`Visibility`].
//!
//! - The look is applied, and the icons follow its icon theme.
//! - The results follow the installed applications as well as the query.
//! - What the model answers is done here ([`Starter`]): Enter asks for an
//!   activation token for the launcher's window (from the key press) and,
//!   in its answer, starts the selected application with it and hides;
//!   Escape, and the window losing the keyboard while shown, hide.
//! - The frequent applications follow the record of starts, the installed
//!   applications and `[general]` `remember-starts` (which the record's
//!   thread follows too). A change to them reaches the model at once while
//!   the launcher is hidden, so that its rows are built before the next
//!   show, and only at the next hide while it is shown. The first, made
//!   with the window, is the model's from the start.
//! - While hidden, the window is prepared for its next show (rinch styles,
//!   lays out and paints it without presenting) at start, at every hide,
//!   and whenever what that show draws changes: see
//!   [`prepare_while_hidden`].

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_component::{Inbox, LiveSettings};
use deskrinch_desktop_entries::{CatalogEntry, Environment};
use deskrinch_icons::Index;
use deskrinch_launcher::{Action, Drawn, Model, Preparation, Visibility, frequent, preparing};
use deskrinch_settings::general::GeneralSettings;
use deskrinch_start_record::Record;
use deskrinch_theme::{Scheme, Theme};
use rinch::prelude::*;
use rinch::reactive::Effect;
use rinch::windows::prepare_current_window;

use crate::start::start;
use crate::starts::{Held, Starts};
use crate::wiring::{self, Wiring};

/// What the launcher runs with.
pub(crate) struct Parts {
    pub(crate) theme: Theme,
    /// `[general]`, for the terminal applications run in.
    pub(crate) general: LiveSettings<GeneralSettings>,
    pub(crate) visibility: Arc<Visibility>,
    /// The window's half of the visibility's hand-over
    /// ([`Visibility::new`]).
    pub(crate) window: Inbox<bool>,
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
        window,
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
    let shown = window.into_signal(visibility.is_shown());
    let record = record.into_signal();

    icons.follow_theme(applied.icon_theme);
    let index = icons.into_signal();

    // The applications changed: search them again for the same query.
    Effect::new(move || {
        let entries = entries.get();
        untracked(|| model.update(|m| m.refresh(entries.iter().map(|e| &e.entry))));
    });

    follow_frequent(&Frequent {
        starts: &starts,
        record,
        general,
        entries,
        model,
        shown,
    });

    let starter = Starter {
        visibility: Arc::clone(&visibility),
        entries,
        general,
        env,
        starts,
    };
    let content = wiring::wire(
        __scope,
        Wiring {
            model,
            entries,
            index,
            visibility,
            shown,
            made_shown,
            act: Rc::new(move |action| starter.act(action)),
        },
    );
    prepare_while_hidden(model, index, applied.scheme, shown);
    content
}

/// Has rinch prepare the hidden window for its next show as
/// [`Preparation`] says: at start, at every hide, and whenever what that
/// show draws changes while the launcher is hidden (the frequent
/// applications, the icon theme's index, the look). The request is queued
/// and runs on the event loop's next turn, after this turn's changes are in
/// the document: a hide's clearing of the model included.
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
        let changes = preparation.wants(drawn, shown.get());
        if !changes.is_empty() {
            tracing::info!("{}", preparing(&changes));
            prepare_current_window();
        }
    });
}

/// What the frequent applications follow, and the model they go to.
struct Frequent<'a> {
    starts: &'a Starts,
    record: Signal<Arc<Record>>,
    general: Signal<GeneralSettings>,
    entries: Memo<Arc<Vec<Arc<CatalogEntry>>>>,
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
        model,
        shown,
    } = parts;
    let remember = Memo::new(move || general.with(|g| g.remember_starts));
    let starts = starts.clone();
    Effect::new(move || starts.remember(remember.get()));
    let frequent = Memo::new(move || {
        if !remember.get() {
            return Vec::new();
        }
        let entries = entries.get();
        record.with(|record| frequent(record, entries.iter().map(|e| &e.entry)))
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

/// What doing what the model answers needs.
#[derive(Clone)]
struct Starter {
    visibility: Arc<Visibility>,
    entries: Memo<Arc<Vec<Arc<CatalogEntry>>>>,
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
                let Some(entry) = self
                    .entries
                    .get()
                    .iter()
                    .find(|e| e.entry.id == id)
                    .map(Arc::clone)
                else {
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
