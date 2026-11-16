//! The daemon's root: the live notifications as a signal, and the column's
//! window while any shows.
//!
//! The root is the document of an app with no window of its own
//! (`App::windowless`): with no notification there is no window at all, so
//! nothing is drawn and nothing wakes. The first notification makes the
//! column's window ([`surface`]); the last one's closing takes it away.
//!
//! The column holds a [`popup`] per live notification, the newest at the
//! top: the model's order (first shown first), drawn from the bottom up.
//! Each popup is its notification's id, and reads its content from the
//! state, so a replacement redraws a popup in its place and a closing
//! removes only its own.

use std::sync::Arc;

use deskrinch_applications::{Icons, Installed};
use deskrinch_notification_service::{Handle, NotificationId};
use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::feed::Feed;
use crate::popup::{self, Shared};
use crate::{ending, style, surface};

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
            ending::service_ended();
        }
    });
    let shared = Shared {
        handle,
        state,
        index: icons.signal(),
        applications: installed.into_signal(),
    };
    let ids = Memo::new(move || {
        state.with(|state| {
            state
                .live
                .iter()
                .rev()
                .map(|n| n.id)
                .collect::<Vec<NotificationId>>()
        })
    });
    let showing = Memo::new(move || !ids.get().is_empty());
    let sheet = style::sheet();
    rsx! {
        div {
            if showing.get() {
                Window {
                    title: surface::TITLE.to_owned(),
                    borderless: true,
                    transparent: true,
                    app_id: Some(surface::NAMESPACE.to_owned()),
                    wayland_layer: Some(surface::layer_surface()),
                    size_to_content: Some(surface::sizing()),
                    style { {sheet.clone()} }
                    div {
                        class: "column",
                        for id in ids.get() {
                            div { key: id, {popup::popup(__scope, id, &shared)} }
                        }
                    }
                }
            }
        }
    }
}
