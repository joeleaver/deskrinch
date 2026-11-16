//! The daemon's root: the live notifications as a signal, and the column's
//! window while any shows.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): with no notification there is no window at all, so
//! nothing is drawn and nothing wakes. The first notification makes the
//! column's window ([`surface`]); the last one's closing takes it away. As
//! the root is built the daemon says so ([`UP_LINE`]): from then on rinch's
//! event loop runs.
//!
//! The window's content ([`view`]) holds a popup per live notification, in
//! [`fit::order`](crate::fit::order)'s order: the critical ones first, then the others, the
//! newest first within each. The popups' container wraps: those that fit
//! whole are shown, the rest wrap out of the window and wait ([`Column`]),
//! and a line under the container says how many wait. Each popup is its
//! notification's id, and reads its content from the state, so a
//! replacement redraws a popup in its place and a closing removes only its
//! own.

use deskrinch_applications::{Icons, Installed};
use deskrinch_component::Inbox;
use deskrinch_notification_service::Handle;
use deskrinch_theme::Theme;
use rinch::InputRegion;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::column::Column;
use crate::feed::State;
use crate::policy::WIDTH;
use crate::popup::Shared;
use crate::{surface, view};

/// What the daemon says, at info level, once its root is built and rinch's
/// event loop runs.
///
/// Where that is true, in rinch (at 689f87c3a): `App::try_run` reaches
/// `try_run_rinch_with_window_props_and_menu`
/// (`crates/rinch/src/shell/rinch_runtime/entry.rs`), which makes the event
/// loop (`overlay_window::new_event_loop`) and then runs it; a windowless
/// app's root is built only from inside the running loop, in its
/// `ApplicationHandler::can_create_surfaces`
/// (`crates/rinch/src/shell/rinch_runtime.rs`), through `start_surfaces`
/// and `mount_windowless_root`
/// (`crates/rinch/src/shell/rinch_runtime/open_windows.rs`). So the line,
/// said as the root is built, comes after the display connection is made
/// and in use.
pub(crate) const UP_LINE: &str = deskrinch_shell::NOTIFICATIONS_UP_LINE;

/// What the daemon's UI runs with.
pub(crate) struct Parts {
    /// The feed's inbox, turned here into the UI's signal of the state.
    pub(crate) inbox: Inbox<State>,
    pub(crate) handle: Handle,
    pub(crate) theme: Theme,
    pub(crate) installed: Installed,
    pub(crate) icons: Icons,
}

/// The daemon's UI, until the app ends.
#[component]
pub(crate) fn notifications(parts: Parts) -> NodeHandle {
    let Parts {
        inbox,
        handle,
        theme,
        installed,
        icons,
    } = parts;
    let applied = theme.apply();
    icons.follow_theme(applied.icon_theme);
    let state = inbox.into_signal(State::default());
    Effect::new(move || {
        if state.with(|state| state.ended) {
            // main reads it from the feed, and ends with a failure.
            exit();
        }
    });
    let order = view::order(state);
    let column = Column::new(handle.clone(), order);
    let shared = Shared {
        handle,
        state,
        index: icons.into_signal(),
        applications: installed.into_signal(),
        column: column.clone(),
    };
    let showing = Memo::new(move || !order.get().is_empty());
    tracing::info!("{UP_LINE}");
    rsx! {
        div {
            if showing.get() {
                Window {
                    title: surface::TITLE.to_owned(),
                    borderless: true,
                    transparent: true,
                    app_id: Some(surface::NAMESPACE.to_owned()),
                    wayland_layer: Some(surface::layer_surface()),
                    width: WIDTH,
                    height: surface::FIRST_HEIGHT,
                    input_region: InputRegion::Marked,
                    {view::content(__scope, order, &shared)}
                }
            }
        }
    }
}
