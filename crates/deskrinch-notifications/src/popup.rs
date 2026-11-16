//! One popup: a live notification as the column draws it.
//!
//! - A header: the icon ([`picture`](crate::picture)), the sending
//!   application's name, and a close button, which dismisses it.
//! - The summary, one line, and the body, its styled runs drawn as text
//!   (bold, italic, underlined): the service parsed the sender's markup, so
//!   nothing a sender wrote is ever markup here. A long body is cut short
//!   after [`BODY_LINES`](crate::policy::BODY_LINES) lines.
//! - The actions ([`actions`](crate::actions)); a click on the summary or
//!   body invokes the default one, when there is one.
//! - A critical notification has the theme's red border.
//!
//! While the pointer is over a popup its expiry waits (the service's
//! pause); it goes on when the pointer leaves. A popup is its notification's
//! id: when the sender replaces the notification, the popup stays and draws
//! the new content, and stays paused if it was.

use std::sync::Arc;

use deskrinch_applications::{Applications, icon_scale};
use deskrinch_icons::Index;
use deskrinch_notification_service::{Action, Handle, Notification, NotificationId, Span, Urgency};
use rinch::prelude::*;

use crate::actions::{self, Button};
use crate::feed::State;
use crate::picture::Wanted;
use crate::policy::ICON_SIZE;

/// What every popup reads.
#[derive(Clone)]
pub(crate) struct Shared {
    pub(crate) handle: Handle,
    pub(crate) state: Signal<State>,
    pub(crate) index: Signal<Option<Arc<Index>>>,
    pub(crate) applications: Signal<Arc<Applications>>,
}

/// The popup of notification `id`.
pub(crate) fn popup(__scope: &mut RenderScope, id: NotificationId, shared: &Shared) -> NodeHandle {
    let state = shared.state;
    let notification =
        Memo::new(move || state.with(|state| state.live.iter().find(|n| n.id == id).cloned()));
    let app = part(notification, |n| n.app_name.clone());
    let summary = part(notification, |n| n.summary.clone());
    let spans = part(notification, |n| n.body.spans().to_vec());
    let buttons = part(notification, actions::buttons);
    let critical = part(notification, |n| n.urgency == Urgency::Critical);
    let has_default = part(notification, |n| n.default_action().is_some());
    let icon = icon_path(notification, shared);

    let (pause, resume, dismiss, open) = (
        shared.handle.clone(),
        shared.handle.clone(),
        shared.handle.clone(),
        shared.handle.clone(),
    );
    let invoke = shared.handle.clone();
    let index = shared.index;
    rsx! {
        div {
            class: {move || if critical.get() { "popup critical" } else { "popup" }},
            onmouseenter: move || {
                // Not live any more: nothing to pause.
                let _gone = pause.pause(id);
            },
            onmouseleave: move || {
                let _gone = resume.resume(id);
            },
            div {
                class: "header",
                if let Some(src) = icon.get() {
                    img { class: "icon", src: src }
                }
                span { class: "app", {move || app.get()} }
                button {
                    class: "close",
                    title: "Dismiss",
                    onclick: move || {
                        let _gone = dismiss.dismiss(id);
                    },
                    "×"
                }
            }
            div {
                class: {move || if has_default.get() { "content default" } else { "content" }},
                onclick: move || {
                    if has_default.get() {
                        actions::invoke(open.clone(), id, Action::DEFAULT_KEY.to_owned());
                    }
                },
                div {
                    class: "text",
                    span { class: "summary", {move || summary.get()} }
                    div {
                        class: "body",
                        for (at, run) in spans.get().into_iter().enumerate() {
                            let style = span_style(&run);
                            let text = run.text;
                            span { key: at, style: style, {text.clone()} }
                        }
                    }
                }
            }
            if !buttons.get().is_empty() {
                div {
                    class: "actions",
                    for button in buttons.get() {
                        {action_button(__scope, button, &invoke, id, index)}
                    }
                }
            }
        }
    }
}

/// A part of the popup's notification, `f` of it, followed; the part's
/// default once it is not live.
fn part<T>(
    notification: Memo<Option<Arc<Notification>>>,
    f: impl Fn(&Notification) -> T + 'static,
) -> Memo<T>
where
    T: Clone + Default + PartialEq + 'static,
{
    Memo::new(move || notification.get().as_deref().map(&f).unwrap_or_default())
}

/// The file the popup's icon is drawn from, as the theme's index, the
/// installed applications and the window's scale change.
fn icon_path(
    notification: Memo<Option<Arc<Notification>>>,
    shared: &Shared,
) -> Memo<Option<String>> {
    let (index, applications) = (shared.index, shared.applications);
    let wanted = Memo::new(move || notification.get().as_deref().map(Wanted::of));
    Memo::new(move || {
        let wanted = wanted.get()?;
        let index = index.get()?;
        let scale = icon_scale(scale_factor());
        applications.with(|applications| wanted.path(&index, applications, ICON_SIZE, scale))
    })
}

/// One action's button: its icon, when it has one the theme has, else its
/// label.
fn action_button(
    __scope: &mut RenderScope,
    button: Button,
    handle: &Handle,
    id: NotificationId,
    index: Signal<Option<Arc<Index>>>,
) -> NodeHandle {
    let icon = button.icon.clone();
    let src = Memo::new(move || {
        let index = index.get()?;
        let name = icon.as_ref()?;
        let scale = icon_scale(scale_factor());
        deskrinch_applications::path_of_name(&index, name.as_str(), ICON_SIZE, scale)
    });
    let (handle, key, label) = (handle.clone(), button.key.clone(), button.label.clone());
    let title = button.label;
    rsx! {
        button {
            class: "action",
            title: title,
            onclick: move || actions::invoke(handle.clone(), id, key.clone()),
            if let Some(src) = src.get() {
                img { class: "icon", src: src }
            } else {
                {label.clone()}
            }
        }
    }
}

/// The inline style of a run of the body.
fn span_style(span: &Span) -> String {
    let mut style = String::new();
    if span.style.bold {
        style.push_str("font-weight: bold;");
    }
    if span.style.italic {
        style.push_str("font-style: italic;");
    }
    if span.style.underline {
        style.push_str("text-decoration: underline;");
    }
    style
}
