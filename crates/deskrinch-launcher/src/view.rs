//! What the launcher draws: the query field, and under it the results, the
//! selected one marked, each with its icon and name. Presses and releases
//! of the primary button, on a result or anywhere else, go to the model
//! ([`Input::Press`], [`Input::Release`]), which makes a click of them.
//!
//! Only [`ROWS`](deskrinch_launcher::ROWS) results fit; the ones drawn are the model's
//! ([`Model::visible`]), which keeps the selection in sight.

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_desktop_entries::DesktopId;
use deskrinch_icons::Index;
use deskrinch_launcher::{Input, Model, Row};
use rinch::prelude::*;

use crate::icons::{SIZE, path_of};

/// The launcher's content, and its query field (to focus at each show).
pub(crate) fn view(
    __scope: &mut RenderScope,
    model: Signal<Model>,
    index: Signal<Option<Arc<Index>>>,
    on_query: impl Fn(String) + 'static,
    on_key: impl Fn(&str) -> bool + 'static,
    on_pointer: Rc<dyn Fn(Input)>,
) -> (NodeHandle, NodeHandle) {
    let rows = Memo::new(move || model.with(|m| m.rows()[m.visible()].to_vec()));
    let selected = Memo::new(move || model.with(|m| m.selected_row().map(|row| row.id.clone())));
    let field = rsx! {
        input {
            class: "launcher-query",
            r#type: "text",
            placeholder: "Type to search applications",
            style: "width: 100%; box-sizing: border-box; \
                    padding: var(--rinch-spacing-sm) var(--rinch-spacing-md); \
                    font-size: var(--rinch-font-size-lg); \
                    color: var(--rinch-color-text); \
                    background: var(--rinch-color-default); \
                    border: 1px solid var(--rinch-color-default-border); \
                    border-radius: var(--rinch-radius-default);",
            value: {move || model.with(|m| m.query().to_owned())},
            oninput: move |text: String| on_query(text),
            onkeydown: move |key| on_key(&key.key),
        }
    };
    let content = rsx! {
        div {
            class: "launcher",
            onmousedown: pointer(&on_pointer, Input::Press, None),
            onmouseup: pointer(&on_pointer, Input::Release, None),
            style: "width: 100%; height: 100%; box-sizing: border-box; \
                    display: flex; flex-direction: column; \
                    gap: var(--rinch-spacing-sm); \
                    background: var(--rinch-color-body); \
                    color: var(--rinch-color-text); \
                    padding: var(--rinch-spacing-md);",
            {field.clone()}
            div {
                class: "launcher-results",
                style: "display: flex; flex-direction: column; overflow: hidden;",
                for row in rows.get() {
                    div {
                        key: row.id.as_str().to_owned(),
                        {row_view(__scope, &row, selected, index, &on_pointer)}
                    }
                }
            }
        }
    };
    (content, field)
}

/// One result: its icon and name, marked while it is the selected one.
fn row_view(
    __scope: &mut RenderScope,
    row: &Row,
    selected: Memo<Option<DesktopId>>,
    index: Signal<Option<Arc<Index>>>,
    on_pointer: &Rc<dyn Fn(Input)>,
) -> NodeHandle {
    let id = row.id.clone();
    let down = pointer(on_pointer, Input::Press, Some(row.id.clone()));
    let up = pointer(on_pointer, Input::Release, Some(row.id.clone()));
    let is_selected = Memo::new(move || selected.get().as_ref() == Some(&id));
    let icon = icon(__scope, row, index);
    rsx! {
        div {
            class: {move || if is_selected.get() { "launcher-row selected" } else { "launcher-row" }},
            onmousedown: down,
            onmouseup: up,
            style: {move || format!(
                "display: flex; align-items: center; gap: var(--rinch-spacing-md); \
                 padding: var(--rinch-spacing-xs) var(--rinch-spacing-sm); \
                 border-radius: var(--rinch-radius-sm); background: {};",
                if is_selected.get() { "var(--rinch-color-option-selected)" } else { "transparent" }
            )},
            {icon}
            span { {row.name.clone()} }
        }
    }
}

/// A press or release handler giving the model `input(on)`, for the
/// primary button only.
fn pointer(
    on_pointer: &Rc<dyn Fn(Input)>,
    input: fn(Option<DesktopId>) -> Input,
    on: Option<DesktopId>,
) -> impl Fn() + 'static {
    let on_pointer = Rc::clone(on_pointer);
    move || {
        if get_click_context().button == MouseButton::Left {
            on_pointer(input(on.clone()));
        }
    }
}

/// The icon scale for a window scale: the next whole one up.
fn icon_scale(factor: f64) -> u32 {
    match factor {
        f if f > 2.0 => 3,
        f if f > 1.0 => 2,
        _ => 1,
    }
}

/// The row's icon, from the icon theme's index once there is one (its own,
/// else the generic program's: [`path_of`]); an empty square of the same
/// size until then, or when the theme has neither.
fn icon(__scope: &mut RenderScope, row: &Row, index: Signal<Option<Arc<Index>>>) -> NodeHandle {
    let wanted = row.icon.clone();
    let path = Memo::new(move || {
        let index = index.get()?;
        let scale = icon_scale(scale_factor());
        path_of(&index, wanted.as_ref(), scale)
    });
    let square = format!("width: {SIZE}px; height: {SIZE}px; flex: none;");
    let image = square.clone();
    rsx! {
        div {
            style: square,
            if let Some(src) = path.get() {
                img { src: src, style: image.clone() }
            }
        }
    }
}
