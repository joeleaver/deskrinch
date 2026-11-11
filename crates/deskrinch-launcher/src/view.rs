//! What the launcher draws: the query field, and under it the results, the
//! selected one marked, each with its icon and name.
//!
//! Only [`ROWS`] results fit; the ones drawn are the stretch that keeps the
//! selection in sight ([`Model::visible`]).

use std::sync::Arc;

use deskrinch_desktop_entries::DesktopId;
use deskrinch_icons::Index;
use deskrinch_launcher::{Model, Row};
use rinch::prelude::*;

use crate::icons::{SIZE, path_of};

/// How many results are drawn at once.
pub(crate) const ROWS: usize = 8;

/// The launcher's content, and its query field (to focus at each show).
pub(crate) fn view(
    __scope: &mut RenderScope,
    model: Signal<Model>,
    index: Signal<Option<Arc<Index>>>,
    on_query: impl Fn(String) + 'static,
    on_key: impl Fn(&str) -> bool + 'static,
) -> (NodeHandle, NodeHandle) {
    let rows = Memo::new(move || model.with(|m| m.rows()[m.visible(ROWS)].to_vec()));
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
                        {row_view(__scope, &row, selected, index)}
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
) -> NodeHandle {
    let id = row.id.clone();
    let is_selected = Memo::new(move || selected.get().as_ref() == Some(&id));
    let icon = icon(__scope, row, index);
    rsx! {
        div {
            class: {move || if is_selected.get() { "launcher-row selected" } else { "launcher-row" }},
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

/// The icon scale for a window scale: the next whole one up.
fn icon_scale(factor: f64) -> u32 {
    match factor {
        f if f > 2.0 => 3,
        f if f > 1.0 => 2,
        _ => 1,
    }
}

/// The row's icon, from the icon theme's index once there is one; an empty
/// square of the same size until then, or when the theme has none.
fn icon(__scope: &mut RenderScope, row: &Row, index: Signal<Option<Arc<Index>>>) -> NodeHandle {
    let wanted = row.icon.clone();
    let path = Memo::new(move || {
        let index = index.get()?;
        let icon = wanted.as_ref()?;
        let scale = icon_scale(scale_factor());
        path_of(&index, icon, scale)
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
