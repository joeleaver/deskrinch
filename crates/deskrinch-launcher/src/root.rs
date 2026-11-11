//! The launcher's root: the look applied, and the window following
//! [`Visibility`].
//!
//! The window is made hidden (or shown, when the process was started to
//! show it) and is shown and hidden from then on as `Visibility` says,
//! through rinch's `show_current_window` and `hide_current_window`.

use std::sync::Arc;

use deskrinch_theme::Theme;
use rinch::prelude::*;
use rinch::reactive::Effect;
use rinch::windows::{hide_current_window, show_current_window};

use deskrinch_launcher::Visibility;

/// The launcher, following `visibility` and `theme` until it closes; its
/// window was made shown if `made_shown`.
#[component]
pub(crate) fn launcher(theme: Theme, visibility: Arc<Visibility>, made_shown: bool) -> NodeHandle {
    let _scheme = theme.apply();
    // What the window is now: as it was made, until the effect changes it.
    let mut applied = made_shown;
    let shown = visibility.signal();
    Effect::new(move || {
        let wanted = shown.get();
        if wanted != applied {
            if wanted {
                show_current_window();
            } else {
                hide_current_window();
            }
            applied = wanted;
        }
    });
    rsx! {
        div {
            class: "launcher",
            style: "width: 100%; height: 100%; box-sizing: border-box; \
                    background: var(--rinch-color-body); \
                    color: var(--rinch-color-text); \
                    padding: var(--rinch-spacing-md);",
        }
    }
}
