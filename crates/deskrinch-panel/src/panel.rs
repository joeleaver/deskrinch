//! The panel's root: its settings as a signal, the window following them,
//! and what the widgets share.
//!
//! The settings the panel started with made its window. Every later change
//! from the configuration arrives in a `Signal<PanelSettings>`, which the
//! widgets read through the context. A change of edge or height moves the
//! window ([`surface::place`]); anything else leaves it alone.
//!
//! The installed applications and the icon theme's index, followed
//! (`deskrinch-applications`), are the taskbar's icons, also through the
//! context ([`AppIcons`]).

use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_component::LiveSettings;
use deskrinch_desktop_entries::Locale;
use deskrinch_settings::panel::PanelSettings;
use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::bar::Bar;
use crate::surface::{self, Placement};
use crate::widgets::AppIcons;

/// What the panel runs with.
pub(crate) struct Parts {
    pub(crate) live: LiveSettings<PanelSettings>,
    pub(crate) theme: Theme,
    pub(crate) installed: Installed,
    pub(crate) icons: Icons,
    /// The user's locales, for the applications' translated keys.
    pub(crate) locales: Vec<Locale>,
}

/// The panel, following its parts until it closes.
#[component]
pub(crate) fn panel(parts: Parts) -> NodeHandle {
    let Parts {
        live,
        theme,
        installed,
        icons,
        locales,
    } = parts;
    let applied = theme.apply();
    icons.follow_theme(applied.icon_theme);
    create_context(AppIcons {
        applications: installed.into_signal(),
        index: icons.signal(),
        locales: Arc::new(locales),
    });
    // Where the window was made.
    let mut placed = Placement::of(&live.initial);
    let settings = live.into_signal();
    create_context(settings);
    let placement = Memo::new(move || Placement::of(&settings.get()));
    Effect::new(move || {
        let wanted = placement.get();
        if wanted != placed {
            surface::place(wanted);
            placed = wanted;
        }
    });
    rsx! {
        Bar {}
    }
}
