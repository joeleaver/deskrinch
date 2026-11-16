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
//! context ([`AppIcons`]). So are the compositor's windows and workspaces,
//! followed on the panel's one shared connection ([`crate::compositor`]),
//! for the taskbar and the switcher: their followers, for what the widgets
//! ask of the compositor, and the signals of their buttons and runs, each
//! made here, once, from its hand-over's inbox, so that every widget
//! reading it is told of every change and the signal lives as long as the
//! panel. And the panel's one clock's text, which every clock face reads
//! (`widgets`' `clock`).

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_component::{Inbox, LiveSettings};
use deskrinch_settings::panel::PanelSettings;
use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::bar::Bar;
use crate::surface::{self, Placement};
use crate::widgets::{AppIcons, Buttons, FollowedWindows, FollowedWorkspaces, Runs, follow_clock};

/// What the panel runs with.
pub(crate) struct Parts {
    pub(crate) live: LiveSettings<PanelSettings>,
    pub(crate) theme: Theme,
    pub(crate) installed: Installed,
    pub(crate) icons: Icons,
    /// The compositor's windows, for the taskbar.
    pub(crate) windows: FollowedWindows,
    /// The compositor's workspaces, for the switcher.
    pub(crate) workspaces: FollowedWorkspaces,
    /// The taskbar's buttons, as the windows' follower hands them over.
    pub(crate) buttons: Inbox<Buttons>,
    /// The switcher's runs, as the workspaces' follower hands them over.
    pub(crate) runs: Inbox<Runs>,
}

/// The panel, following its parts until it closes.
#[component]
pub(crate) fn panel(parts: Parts) -> NodeHandle {
    let Parts {
        live,
        theme,
        installed,
        icons,
        windows,
        workspaces,
        buttons,
        runs,
    } = parts;
    let applied = theme.apply();
    icons.follow_theme(applied.icon_theme);
    create_context(AppIcons {
        applications: installed.into_signal(),
        index: icons.into_signal(),
    });
    create_context(Rc::new(windows));
    create_context(Rc::new(workspaces));
    create_context(buttons.into_signal(Arc::default()));
    create_context(runs.into_signal(Arc::default()));
    // Where the window was made.
    let mut placed = Placement::of(&live.initial);
    let settings = live.into_signal();
    create_context(settings);
    // The panel's one clock; its faces read the text.
    create_context(follow_clock(settings));
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
