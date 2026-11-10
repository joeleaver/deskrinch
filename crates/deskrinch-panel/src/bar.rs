//! The bar: three regions across the panel, left, centre and right.
//!
//! The left and right regions hug their ends and the centre one sits in the
//! middle of the bar, whatever the others hold: the outer regions take equal
//! shares of the space left over, so the centre is centred on the output,
//! not between the outer regions' contents. Each region lays its widgets out
//! in a row.
//!
//! The clock is at the right end. The taskbar, the tray and the workspace
//! switcher are widgets to come, and each takes its place in a region here.
//!
//! The colours, spacing and type are rinch's theme variables
//! (`--rinch-color-body`, `--rinch-color-text`, `--rinch-spacing-*`,
//! `--rinch-font-size-*`), with the theme the panel starts with
//! (`main.rs`); none is written here.

use rinch::prelude::*;

use crate::widgets::ClockWidget;

/// The style every region shares: a row of widgets, centred vertically.
const REGION: &str = "display: flex; flex-direction: row; align-items: center; \
                      gap: var(--rinch-spacing-sm); height: 100%;";

/// The panel's content: its three regions and their widgets.
#[component]
pub(crate) fn Bar() -> NodeHandle {
    rsx! {
        div {
            class: "bar",
            style: "display: flex; flex-direction: row; align-items: center; \
                    width: 100%; height: 100%; box-sizing: border-box; \
                    padding: 0 var(--rinch-spacing-md); \
                    background: var(--rinch-color-body); \
                    color: var(--rinch-color-text); \
                    font-family: var(--rinch-font-family); \
                    font-size: var(--rinch-font-size-sm);",
            div {
                class: "region left",
                style: {format!("{REGION} flex: 1 1 0; justify-content: flex-start;")},
            }
            div {
                class: "region centre",
                style: {format!("{REGION} flex: 0 0 auto; justify-content: center;")},
            }
            div {
                class: "region right",
                style: {format!("{REGION} flex: 1 1 0; justify-content: flex-end;")},
                ClockWidget {}
            }
        }
    }
}
