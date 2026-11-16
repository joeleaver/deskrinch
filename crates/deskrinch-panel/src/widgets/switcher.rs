//! The workspace switcher: a button for each workspace
//! (`deskrinch-switcher`'s model, followed by [`follow`]), showing its
//! position; the current one marked, and the one under the pointer marked
//! (CSS `:hover`, in the
//! bar's stylesheet: [`crate::widgets::buttons`]). A click asks the
//! compositor to activate a workspace, and the mark moves when it has.
//!
//! The switcher takes at most [`SHARE`] of the bar: its buttons, all of one
//! width, fit in that, and past it the workspaces furthest from the current
//! one are left out, counted at either end ([`deskrinch_switcher::fit`]).
//! Where the compositor offers no workspace protocol, it is not shown at
//! all, and takes no room.
//!
//! It draws only when what it shows changes, or the pointer moves onto or
//! off a button: the runs come as one value per batch of the compositor's
//! changes, what is drawn is a memo of them and of the room, and each item
//! drawn is keyed by its workspace.

mod follow;

pub(crate) use follow::{Following, Runs};

use std::fmt::Display;
use std::rc::Rc;

use deskrinch_switcher::{Button, Fitted, Room, fit};
use deskrinch_workspaces::WorkspaceId;
use rinch::prelude::*;

use crate::bar::BarWidth;
use crate::widgets::buttons;

/// The most of the bar the switcher takes, as a fraction of its width: the
/// taskbar takes what the switcher and the clock leave, and its windows
/// keep at least three quarters of the bar however many workspaces there
/// are.
const SHARE: f32 = 0.25;

/// A button's width, in logical pixels: the taskbar's icon-only button's,
/// a position's digits and padding with room to spare.
const BUTTON: f32 = 28.0;
/// Between two buttons, as in the taskbar.
const GAP: f32 = 4.0;
/// Between two runs of buttons (two groups' workspaces).
const RUN_GAP: f32 = 12.0;
/// A count of the workspaces left out at one end.
const COUNT_WIDTH: f32 = 28.0;

/// One thing drawn, in order.
#[derive(Debug, Clone, PartialEq)]
enum Item<Id> {
    /// A workspace's button.
    Button(Button<Id>),
    /// How many of a run are left out at one end; `at` keys it.
    Count { left_out: usize, at: String },
    /// The space between two runs, after run `after`.
    Gap { after: usize },
}

impl<Id: Display> Item<Id> {
    /// What keeps an item's element across changes.
    fn key(&self) -> String {
        match self {
            Self::Button(button) => button.id.to_string(),
            Self::Count { at, .. } => at.clone(),
            Self::Gap { after } => format!("gap {after}"),
        }
    }
}

/// The items of `fitted` runs, in order.
fn items<Id>(fitted: Vec<Fitted<Id>>) -> Vec<Item<Id>> {
    let mut items = Vec::new();
    for (index, run) in fitted.into_iter().enumerate() {
        if index > 0 {
            items.push(Item::Gap { after: index - 1 });
        }
        if run.before > 0 {
            items.push(Item::Count {
                left_out: run.before,
                at: format!("before {index}"),
            });
        }
        items.extend(run.shown.into_iter().map(Item::Button));
        if run.after > 0 {
            items.push(Item::Count {
                left_out: run.after,
                at: format!("after {index}"),
            });
        }
    }
    items
}

/// The switcher widget, at the start of the bar.
#[component]
pub(crate) fn SwitcherWidget() -> NodeHandle {
    let following: Rc<Following> = use_context();
    let runs: Signal<Runs> = use_context();
    let BarWidth(bar_width) = use_context();
    let drawn = Memo::new(move || {
        let room = Room {
            width: bar_width.get() * SHARE,
            button: BUTTON,
            gap: GAP,
            run_gap: RUN_GAP,
            count_width: COUNT_WIDTH,
        };
        runs.with(|runs| items(fit(runs, room)))
    });
    let keys = Memo::new(move || drawn.get().iter().map(Item::key).collect::<Vec<_>>());
    let shown = Memo::new(move || !keys.get().is_empty());
    rsx! {
        div {
            class: "switcher",
            style: {move || format!(
                "display: {}; flex-direction: row; align-items: center; \
                 gap: {GAP}px; flex: none; height: 100%;",
                if shown.get() { "flex" } else { "none" }
            )},
            for key in keys.get() {
                div {
                    key: key.clone(),
                    style: "display: flex; height: 100%; align-items: center;",
                    {item_view(__scope, key.clone(), drawn, &following)}
                }
            }
        }
    }
}

/// The item keyed `key`, drawn from `drawn` as it changes: a button, a
/// count, or the space between two runs.
fn item_view(
    __scope: &mut RenderScope,
    key: String,
    drawn: Memo<Vec<Item<WorkspaceId>>>,
    following: &Rc<Following>,
) -> NodeHandle {
    let item = Memo::new(move || drawn.get().into_iter().find(|item| item.key() == key));
    let following = Rc::clone(following);
    rsx! {
        div {
            class: {move || item.get().as_ref().map_or("", item_class)},
            style: {move || item.get().as_ref().map(item_style).unwrap_or_default()},
            onclick: move || {
                if let Some(Item::Button(button)) = item.get() {
                    following.activate(button.id);
                }
            },
            {move || item.get().as_ref().map(item_text).unwrap_or_default()}
        }
    }
}

/// An item's classes: a button's, marked when current; none otherwise.
fn item_class<Id>(item: &Item<Id>) -> &'static str {
    match item {
        Item::Button(button) => buttons::classes(button.current),
        Item::Count { .. } | Item::Gap { .. } => "",
    }
}

/// What an item shows: a button's position, a count's number.
fn item_text<Id>(item: &Item<Id>) -> String {
    match item {
        Item::Button(button) => button.label.clone(),
        Item::Count { left_out, .. } => format!("+{left_out}"),
        Item::Gap { .. } => String::new(),
    }
}

/// An item's style: its width, and a button's shape; its background is the
/// bar's stylesheet's.
fn item_style<Id>(item: &Item<Id>) -> String {
    let width = match item {
        Item::Button(_) => BUTTON,
        Item::Count { .. } => COUNT_WIDTH,
        Item::Gap { .. } => RUN_GAP - 2.0 * GAP,
    };
    let look = match item {
        Item::Button(_) => "border-radius: var(--rinch-radius-sm); cursor: default;",
        Item::Count { .. } => "color: var(--rinch-color-dimmed);",
        Item::Gap { .. } => "",
    };
    format!(
        "display: flex; align-items: center; justify-content: center; flex: none; \
         box-sizing: border-box; width: {width}px; height: 100%; {look}"
    )
}

#[cfg(test)]
#[path = "switcher_tests.rs"]
mod tests;
