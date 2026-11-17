//! The taskbar: a button for each open window (`deskrinch-taskbar`'s
//! model, followed by [`follow`]), with its application's icon and its
//! title cut to fit, marked while active, dimmed while minimized, marked
//! while it asks for attention, and marked while the pointer is over it
//! (CSS `:hover`, in the bar's stylesheet: [`crate::widgets::buttons`]). A
//! click of the primary button activates its window, or minimizes the
//! active one; the middle button closes it.
//!
//! Each bar's taskbar shows the windows on its own output, from the one
//! list of buttons the panel follows: a memo over that list, the bar's
//! output, the bars there are, and the [`GraceClock`]. The rule is
//! [`deskrinch_taskbar::on_bar`](mod@deskrinch_taskbar::on_bar)'s: a window
//! whose outputs have no bar is on every bar, and one the compositor has
//! not yet placed waits a short grace, then is on every bar until it is
//! placed. A window's change wakes only the bars whose list it changes.
//!
//! The buttons share the room the bar leaves the taskbar (its width, from
//! rinch's `bounds_signal`, refreshed after each layout), shrink to their
//! icons when there is too little, and the oldest that are not active are
//! left out, counted at the end, when even the icons do not fit
//! ([`deskrinch_taskbar::fit`]).
//!
//! It draws only when what it shows changes, or the pointer moves onto or
//! off a button (one frame each, a paint without layout): the buttons come
//! as one value per change of the window list, each button's parts are
//! memos of it, and the list is keyed by window.

mod button;
mod follow;
mod grace;

pub(crate) use button::AppIcons;
pub(crate) use follow::{Buttons, Following};
pub(crate) use grace::{GraceClock, follow as follow_graces};

use std::rc::Rc;
use std::sync::Arc;

use deskrinch_taskbar::{Bars, Room, fit, shown_on};
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::widgets::taskbar::button::{ButtonSizes, button_view, pointer};

/// The sizes buttons come in, in logical pixels.
const SIZES: ButtonSizes = ButtonSizes {
    icon: 16,
    titled_min: 96.0,
    titled_max: 200.0,
    padding: 6.0,
    gap: 4.0,
    count_width: 36.0,
};

/// The name of the output a bar is on (`None` when it has none), which
/// each bar provides as a context for its taskbar.
#[derive(Debug, Clone)]
pub(crate) struct BarOutput(pub(crate) Option<String>);

/// The bars there are, which the panel's root provides as a context: one
/// on every output, counted, and named by the outputs that have a name. A
/// memo, so an output moved or rescaled wakes no taskbar.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BarOutputs(pub(crate) Memo<Bars>);

/// The taskbar widget: the buttons of its bar's output
/// ([`deskrinch_taskbar::shown_on`]), in the room the bar gives it.
#[component]
pub(crate) fn TaskbarWidget() -> NodeHandle {
    let following: Rc<Following> = use_context();
    let all: Signal<Buttons> = use_context();
    let BarOutput(output) = use_context();
    let BarOutputs(bars) = use_context();
    let GraceClock(clock) = use_context();
    let buttons = Memo::new(move || {
        let bars = bars.get();
        let now = clock.get();
        all.with(|all| Arc::new(shown_on(all, output.as_deref(), &bars, now)))
    });
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
        fit(&buttons.get(), room)
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
