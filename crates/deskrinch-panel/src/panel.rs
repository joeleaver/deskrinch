//! The panel's root: its settings as a signal, a bar on every output, and
//! what the bars share.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): it holds one `Window` per output, a row of a `for`
//! over rinch's `output_ids()`, keyed by the id, each a bar
//! ([`output_bar`]). The row's item is the output's id alone, not rinch's
//! whole `Output`, which changes with the output's position, size and scale:
//! an output moved, rescaled or turned keeps its row and its window, whose
//! layer surface the compositor reconfigures; only an output coming or
//! going adds or removes a row ([`surface`]'s "When a window closes").
//!
//! Every change of the settings arrives in a `Signal<PanelSettings>`, which
//! the widgets read through the context. A change of edge or height moves
//! every bar, each from an effect of its own window ([`surface::place`]);
//! anything else leaves them alone.
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
//! (`widgets`' `clock`). And the bars there are, which each bar's taskbar
//! reads to show the windows on its own output ([`BarOutputs`]), with the
//! one timer for the new windows' graces (`widgets`' `taskbar`).

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_component::{Inbox, LiveSettings};
use deskrinch_settings::panel::PanelSettings;
use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;
use rinch::{OutputId, WindowHandle};

use crate::bar::Bar;
use crate::surface::{self, BarSurface, Placement};
use crate::widgets::{
    AppIcons, BarOutputs, Buttons, FollowedWindows, FollowedWorkspaces, Runs, follow_clock,
    follow_graces,
};
use deskrinch_taskbar::Bars;

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
    let buttons = buttons.into_signal(Arc::default());
    create_context(buttons);
    create_context(follow_graces(buttons));
    create_context(runs.into_signal(Arc::default()));
    let settings = live.into_signal();
    create_context(settings);
    // The panel's one clock; its faces read the text.
    create_context(follow_clock(settings));
    create_context(BarOutputs(Memo::new(bars_there_are)));
    let placement = Memo::new(move || Placement::of(&settings.get()));
    bars(__scope, Memo::new(output_ids), placement)
}

/// The bars there are: one on each connected output, named by the
/// outputs' names (an empty name is no name).
fn bars_there_are() -> Bars {
    let outputs = outputs();
    Bars {
        count: outputs.len(),
        named: outputs
            .into_iter()
            .filter_map(|output| output.name)
            .filter(|name| !name.is_empty())
            .collect(),
    }
}

/// A bar on each of `outputs`, at `placement`: one `Window` per output, a
/// row keyed by the output's id. The root hands it rinch's `output_ids()`
/// and the settings' placement; a test may hand it its own.
pub(crate) fn bars(
    __scope: &mut RenderScope,
    outputs: Memo<Vec<OutputId>>,
    placement: Memo<Placement>,
) -> NodeHandle {
    rsx! {
        div {
            for id in outputs.get() {
                div {
                    key: id,
                    {bar_window(__scope, id, outputs, placement)}
                }
            }
        }
    }
}

/// The window of the bar on the output whose id is `output`, one of
/// `outputs`, drawn with its row: opened at the placement as it is now,
/// which its bar is handed as what its window was opened with.
fn bar_window(
    __scope: &mut RenderScope,
    output: OutputId,
    outputs: Memo<Vec<OutputId>>,
    placement: Memo<Placement>,
) -> NodeHandle {
    let opened = surface::bar_surface(untracked(|| placement.get()), output);
    let height = opened.height;
    let layer = opened.layer.clone();
    rsx! {
        Window {
            title: surface::TITLE.to_owned(),
            width: surface::FIRST_WIDTH,
            height: height,
            borderless: true,
            app_id: Some(surface::NAMESPACE.to_owned()),
            wayland_layer: Some(layer),
            on_closed: move || surface::closed(output),
            {output_bar(__scope, output, outputs, placement, opened)}
        }
    }
}

/// The bar on the output whose id is `output`, inside that output's window:
/// while `output` is among `outputs`, it follows `placement`, moving its
/// window through the window's handle when the edge or height changes
/// ([`surface::place`]). The handle is the
/// window's own: rinch's current window while its content is built, which
/// is when this runs.
fn output_bar(
    __scope: &mut RenderScope,
    output: OutputId,
    outputs: Memo<Vec<OutputId>>,
    placement: Memo<Placement>,
    opened: BarSurface,
) -> NodeHandle {
    let window = WindowHandle::current();
    if window.is_none() {
        tracing::error!(
            "the bar on output {output:?} has no window handle at its mount, and cannot follow the \
             edge and height"
        );
    }
    // Compared first against what the window was opened with (`opened`):
    // its row read the placement a turn before this mount, and the settings
    // may have changed in between, so the first run sends the difference;
    // rinch queues it behind the window's opening.
    let mut applied = opened;
    Effect::new(move || {
        let at = placement.get();
        // The window's document, and this effect with it, goes when its
        // close is served, not when the row goes: an output that goes in
        // the turn its placement changes leaves this effect a run, in
        // either order of the two, with the window closing. Read untracked:
        // whether the output is there decides, its going does not move the
        // bar.
        let connected = untracked(|| outputs.get()).contains(&output);
        if let Some(window) = window
            && let Some(sent) =
                surface::to_send(&applied, surface::bar_surface(at, output), connected)
        {
            surface::place(window, output, &sent, at);
            applied = sent;
        }
    });
    // An output's name is its connector's, fixed while it is connected,
    // so read once; untracked, so the bar is not made again at every change
    // of any output.
    let name = untracked(|| rinch::prelude::output(output)).and_then(|output| output.name);
    rsx! {
        Bar { output: name }
    }
}
