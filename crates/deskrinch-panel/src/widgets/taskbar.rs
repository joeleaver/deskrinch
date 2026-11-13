//! The taskbar: a button for each open window (`deskrinch-taskbar`'s
//! model, followed by [`follow`]), with its application's icon and its
//! title cut to fit, marked while active, dimmed while minimized, marked
//! while it asks for attention. A click of the primary button activates its
//! window, or minimizes the active one; the middle button closes it.
//!
//! The buttons share the room the bar leaves the taskbar (its width, from
//! rinch's `bounds_signal`, refreshed after each layout), shrink to their
//! icons when there is too little, and the oldest that are not active are
//! left out, counted at the end, when even the icons do not fit
//! ([`deskrinch_taskbar::fit`]).
//!
//! It draws only when what it shows changes: the buttons come as one value
//! per change of the window list, each button's parts are memos of it, and
//! the list is keyed by window.

mod button;
mod follow;

pub(crate) use button::AppIcons;

use std::rc::Rc;

use deskrinch_taskbar::{Room, fit};
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::widgets::taskbar::button::{ButtonSizes, button_view, pointer};
use crate::widgets::taskbar::follow::Following;

/// The sizes buttons come in, in logical pixels.
const SIZES: ButtonSizes = ButtonSizes {
    icon: 16,
    titled_min: 96.0,
    titled_max: 200.0,
    padding: 6.0,
    gap: 4.0,
    count_width: 36.0,
};

/// The taskbar widget: it takes the room the bar gives it.
#[component]
pub(crate) fn TaskbarWidget() -> NodeHandle {
    let following = Rc::new(Following::start());
    let buttons = following.signal();
    let width = Signal::new(0.0_f32);
    let fitted = Memo::new(move || {
        let room = Room {
            width: width.get(),
            titled_min: SIZES.titled_min,
            titled_max: SIZES.titled_max,
            icon_only: SIZES.icon_only(),
            gap: SIZES.gap,
            count_width: SIZES.count_width,
        };
        buttons.with(|buttons| fit(buttons, room))
    });
    let shown = Memo::new(move || fitted.get().shown);
    let left_out = Memo::new(move || fitted.get().left_out);
    let form = Memo::new(move || fitted.get().form);
    let strip = rsx! {
        div {
            class: "taskbar",
            style: {format!(
                "display: flex; flex-direction: row; align-items: center; \
                 gap: {}px; flex: 1 1 0; min-width: 0; height: 100%; overflow: hidden;",
                SIZES.gap
            )},
            onmousedown: pointer(&following, None, true),
            onmouseup: pointer(&following, None, false),
            for id in shown.get() {
                div {
                    key: id.get().to_string(),
                    style: "display: flex; height: 100%; align-items: center;",
                    {button_view(__scope, id, buttons, form, &following, SIZES)}
                }
            }
            if left_out.get() > 0 {
                span {
                    class: "taskbar-left-out",
                    style: {format!(
                        "flex: none; width: {}px; text-align: center; \
                         color: var(--rinch-color-dimmed);",
                        SIZES.count_width
                    )},
                    {move || format!("+{}", left_out.get())}
                }
            }
        }
    };
    let bounds = strip.bounds_signal();
    Effect::new(move || {
        let now = bounds.get().width;
        // A new width only when it moved by half a pixel or more: layout
        // refreshes the bounds after every pass.
        if (untracked(|| width.get()) - now).abs() >= 0.5 {
            width.set(now);
        }
    });
    strip
}
