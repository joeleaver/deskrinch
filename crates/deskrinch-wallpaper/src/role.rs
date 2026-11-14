//! Whether the wallpaper can be what a wallpaper must be: a layer surface
//! on the background layer of each output.
//!
//! On a compositor without the layer shell (GNOME), rinch makes a window
//! asked for a layer surface an ordinary window instead, which would cover
//! the desktop. rinch says which roles the display offers before any
//! window opens (`rinch::window::window_roles`: on Linux once its event
//! loop has started, which is before an `App::windowless` root runs). So
//! the root asks first ([`verdict`]); without the layer shell the
//! wallpaper says why, opens nothing, and ends with a failure status
//! ([`refuse`], [`refused`]).

use std::sync::atomic::{AtomicBool, Ordering};

use rinch::prelude::{exit, window_roles};

/// What the display's window roles mean for the wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// A layer surface is there to be had: open the windows.
    Layer,
    /// The compositor offers no layer shell.
    NoLayerShell,
    /// rinch does not know yet, which it always does by the time a
    /// windowless root runs; refused rather than guessed.
    Unknown,
}

/// The verdict on `layer_shell`, whether the display offers a layer
/// surface, as rinch's `window_roles()` says (`None` while it does not
/// know). The field is passed in, not the roles: rinch's `WindowRoles` is
/// read, never built.
pub(crate) fn verdict(layer_shell: Option<bool>) -> Verdict {
    match layer_shell {
        Some(true) => Verdict::Layer,
        Some(false) => Verdict::NoLayerShell,
        None => Verdict::Unknown,
    }
}

/// The verdict on this display.
pub(crate) fn of_display() -> Verdict {
    verdict(window_roles().map(|roles| roles.layer_shell))
}

/// Set once the wallpaper refused to open.
static REFUSED: AtomicBool = AtomicBool::new(false);

/// Says why the wallpaper opens nothing, and ends the app (with a failure
/// status, [`refused`]): it returns, and the caller opens nothing.
pub(crate) fn refuse(verdict: Verdict) {
    let why = match verdict {
        Verdict::Layer => return,
        Verdict::NoLayerShell => {
            "this compositor offers no layer shell (zwlr_layer_shell_v1), so a wallpaper would \
             be an ordinary window over the desktop"
        }
        Verdict::Unknown => "rinch could not say whether this display offers a layer shell",
    };
    tracing::error!("wallpaper: {why}; it opens nothing and exits");
    REFUSED.store(true, Ordering::Relaxed);
    exit();
}

/// Whether the wallpaper refused to open; the process then exits with a
/// failure status.
pub(crate) fn refused() -> bool {
    REFUSED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_display_with_the_layer_shell_gets_windows() {
        assert_eq!(verdict(Some(true)), Verdict::Layer);
        assert_eq!(verdict(Some(false)), Verdict::NoLayerShell);
        assert_eq!(verdict(None), Verdict::Unknown);
    }
}
