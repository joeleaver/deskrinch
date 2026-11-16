//! The bar: the workspace switcher, the taskbar, then the end region.
//!
//! The switcher takes its content's width, at most a share of the bar's
//! (its own rule: [`crate::widgets`]'s switcher), and none where the
//! compositor has no workspaces to show. The taskbar takes the room the
//! other widgets leave: it grows and shrinks with the bar. The end region
//! takes its content's width: the clock at the bar's end (the tray, to come,
//! before it). Each widget is a module of its own under
//! [`widgets`](crate::widgets); the bar only places them, and holds what
//! they share: the buttons' stylesheet ([`buttons::STYLESHEET`]) and its own
//! width ([`BarWidth`]).
//!
//! The colours, spacing and type are rinch's theme variables
//! (`--rinch-color-body`, `--rinch-color-text`, `--rinch-spacing-*`,
//! `--rinch-font-size-*`) of the shell's look (`deskrinch-theme`), dark or
//! light; none is written here.

use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::widgets::{ClockWidget, SwitcherWidget, TaskbarWidget, buttons};

/// The bar's width in logical pixels, as its last layout left it, which the
/// widgets that take a share of the bar read from the context: 0 until the
/// first layout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BarWidth(pub(crate) Signal<f32>);

/// The panel's content.
#[component]
pub(crate) fn Bar() -> NodeHandle {
    let width = Signal::new(0.0_f32);
    create_context(BarWidth(width));
    let bar = rsx! {
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
            style { {buttons::STYLESHEET} }
            SwitcherWidget {}
            TaskbarWidget {}
            div {
                class: "region end",
                style: "display: flex; flex-direction: row; align-items: center; \
                        gap: var(--rinch-spacing-sm); height: 100%; flex: 0 0 auto;",
                ClockWidget {}
            }
        }
    };
    let bounds = bar.bounds_signal();
    Effect::new(move || {
        let now = bounds.get().width;
        // A new width only when it moved by half a pixel or more: layout
        // refreshes the bounds after every pass.
        if (untracked(|| width.get()) - now).abs() >= 0.5 {
            width.set(now);
        }
    });
    bar
}
