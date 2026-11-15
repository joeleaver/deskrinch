//! The bar: the taskbar, then the end region.
//!
//! The taskbar takes the room the other widgets leave: it grows and shrinks
//! with the bar. The end region takes its content's width: the clock at the
//! bar's end (the tray, to come, before it). Each widget is a module of its
//! own under [`widgets`](crate::widgets); the bar only places them.
//!
//! The colours, spacing and type are rinch's theme variables
//! (`--rinch-color-body`, `--rinch-color-text`, `--rinch-spacing-*`,
//! `--rinch-font-size-*`) of the shell's look (`deskrinch-theme`), dark or
//! light; none is written here.

use rinch::prelude::*;

use crate::widgets::{ClockWidget, TaskbarWidget};

/// The panel's content.
#[component]
pub(crate) fn Bar() -> NodeHandle {
    rsx! {
        div {
            class: "bar",
            style: "display: flex; flex-direction: row; align-items: center; \
                    gap: var(--rinch-spacing-md); \
                    width: 100%; height: 100%; box-sizing: border-box; \
                    padding: 0 var(--rinch-spacing-md); \
                    background: var(--rinch-color-body); \
                    color: var(--rinch-color-text); \
                    font-family: var(--rinch-font-family); \
                    font-size: var(--rinch-font-size-sm);",
            TaskbarWidget {}
            div {
                class: "region end",
                style: "display: flex; flex-direction: row; align-items: center; \
                        gap: var(--rinch-spacing-sm); height: 100%; flex: 0 0 auto;",
                ClockWidget {}
            }
        }
    }
}
