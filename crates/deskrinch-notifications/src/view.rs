//! The column's window's content: the stylesheet, and the column holding
//! a [`popup`] per live notification in [`fit::order`]'s order ([`order`]),
//! with the count line under the popups' container while some wait.
//!
//! The root puts it in the column's window ([`root`](crate::root)); the
//! harness tests mount the same content in a test window of the same
//! props, at the heights they choose (its `tests` module).

#[cfg(test)]
mod tests;

use deskrinch_notification_service::{NotificationId, Urgency};
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::column::{Column, Room};
use crate::feed::State;
use crate::fit::{self, Place};
use crate::popup::{self, Shared};
use crate::style;

/// The live notifications of `state` in the column's order
/// ([`fit::order`]), followed.
pub(crate) fn order(state: Signal<State>) -> Memo<Vec<NotificationId>> {
    Memo::new(move || {
        state.with(|state| {
            let live: Vec<(NotificationId, bool)> = state
                .live
                .iter()
                .map(|n| (n.id, n.urgency == Urgency::Critical))
                .collect();
            fit::order(&live)
        })
    })
}

/// The window's content for the notifications in `order`, each popup
/// reading what `shared` holds.
pub(crate) fn content(
    __scope: &mut RenderScope,
    order: Memo<Vec<NotificationId>>,
    shared: &Shared,
) -> NodeHandle {
    let container: Signal<Option<NodeHandle>> = Signal::new(None);
    let column: Signal<Option<NodeHandle>> = Signal::new(None);
    report_room(container, column, &shared.column);
    let counted = shared.column.clone();
    let waiting = Memo::new(move || counted.waiting_count());
    let sheet = style::sheet();
    let shared = shared.clone();
    rsx! {
        div {
            class: "window",
            style { {sheet.clone()} }
            div {
                ref: column,
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

/// Reports to `state`, after each layout, where the popups' container (the
/// room, once `container` holds it) and the column around it (once
/// `column` holds it) are: both from one layout, in one report.
fn report_room(
    container: Signal<Option<NodeHandle>>,
    column: Signal<Option<NodeHandle>>,
    state: &Column,
) {
    let state = state.clone();
    Effect::new(move || {
        let (Some(container), Some(column)) = (container.get(), column.get()) else {
            return;
        };
        let room = place_of(&container);
        if room.height > 0.0 {
            state.room(Room {
                container: room,
                column: place_of(&column),
            });
        }
    });
}

/// Where the layout last put `node`, followed.
fn place_of(node: &NodeHandle) -> Place {
    let bounds = node.bounds_signal().get();
    Place {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    }
}
