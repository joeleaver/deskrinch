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
//! The column holds a [`popup`] per live notification, in [`fit::order`]'s
//! order: the critical ones first, then the others, the newest first within
//! each. The popups' container wraps: those that fit whole are shown, the
//! rest wrap out of the window and wait ([`Column`]), and a line under the
//! container says how many wait. Each popup is its notification's id, and
//! reads its content from the state, so a replacement redraws a popup in
//! its place and a closing removes only its own.

use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_notification_service::{Handle, NotificationId, Urgency};
use deskrinch_theme::Theme;
use rinch::InputRegion;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::column::Column;
use crate::feed::Feed;
use crate::fit::{self, Place};
use crate::policy::WIDTH;
use crate::popup::{self, Shared};
use crate::{style, surface};

/// What the daemon says, at info level, once its root is built and rinch's
/// event loop runs.
pub(crate) const UP_LINE: &str = deskrinch_shell::NOTIFICATIONS_UP_LINE;

/// What the daemon's UI runs with.
pub(crate) struct Parts {
    pub(crate) feed: Arc<Feed>,
    pub(crate) handle: Handle,
    pub(crate) theme: Theme,
    pub(crate) installed: Installed,
    pub(crate) icons: Icons,
}

/// The daemon's UI, until the app ends.
#[component]
pub(crate) fn notifications(parts: Parts) -> NodeHandle {
    let Parts {
        feed,
        handle,
        theme,
        installed,
        icons,
    } = parts;
    let applied = theme.apply();
    icons.follow_theme(applied.icon_theme);
    let state = feed.signal();
    Effect::new(move || {
        if state.with(|state| state.ended) {
            // main reads it from the feed, and ends with a failure.
            exit();
        }
    });
    let order = Memo::new(move || {
        state.with(|state| {
            let live: Vec<(NotificationId, bool)> = state
                .live
                .iter()
                .map(|n| (n.id, n.urgency == Urgency::Critical))
                .collect();
            fit::order(&live)
        })
    });
    let column = Column::new(handle.clone(), order);
    let shared = Shared {
        handle,
        state,
        index: icons.signal(),
        applications: installed.into_signal(),
        column: column.clone(),
    };
    let showing = Memo::new(move || !order.get().is_empty());
    let container: Signal<Option<NodeHandle>> = Signal::new(None);
    report_room(container, &column);
    let counted = column.clone();
    let waiting = Memo::new(move || counted.waiting_count());
    let sheet = style::sheet();
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
                    style { {sheet.clone()} }
                    div {
                        class: "column",
                        div {
                            ref: container,
                            class: "popups",
                            for id in order.get() {
                                div { key: id, class: "slot", {popup::popup(__scope, id, &shared)} }
                            }
                        }
                        if waiting.get() > 0 {
                            div { class: "count", {move || format!("{} more", waiting.get())} }
                        }
                    }
                }
            }
        }
    }
}

/// Reports to `column`, after each layout, where the popups' container (the
/// room, once `container` holds it) is.
fn report_room(container: Signal<Option<NodeHandle>>, column: &Column) {
    let column = column.clone();
    Effect::new(move || {
        if let Some(node) = container.get() {
            let bounds = node.bounds_signal().get();
            if bounds.height > 0.0 {
                column.room(Place {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                });
            }
        }
    });
}
