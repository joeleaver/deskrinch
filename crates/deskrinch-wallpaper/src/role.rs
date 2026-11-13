//! Whether the wallpaper's windows are what a wallpaper must be: layer
//! surfaces on the background layer. On a compositor without the layer
//! shell (GNOME), rinch makes each an ordinary window instead, which would
//! cover the desktop; the wallpaper then says why and exits, with a failure
//! status, opening no more.
//!
//! rinch says what a window became only once it exists (rinch issue 2659:
//! nothing answers before a window opens). So each window, as it opens,
//! looks at its surface kind until rinch knows it: a few turns of the event
//! loop at the start, and never again. Should rinch never say, it stops
//! looking after [`LOOKS`] and says so, so that nothing wakes the wallpaper
//! up while nothing changes.

use std::sync::atomic::{AtomicBool, Ordering};

use rinch::prelude::{WindowHandle, WindowSurfaceKind, exit, set_timeout};

/// How often a window looks at its surface kind until rinch knows it.
const LOOK_EVERY_MS: u32 = 50;
/// How many looks a window takes before it gives up: ten seconds' worth.
const LOOKS: u32 = 10_000 / LOOK_EVERY_MS;

/// What a window does after `looks` looks that found nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Looks again.
    LookAgain,
    /// Gives up, and says so.
    GiveUp,
}

/// The next step after `looks` looks without an answer.
pub(crate) fn next(looks: u32) -> Next {
    if looks < LOOKS {
        Next::LookAgain
    } else {
        Next::GiveUp
    }
}

/// Set once a window turned out not to be a layer surface.
static REFUSED: AtomicBool = AtomicBool::new(false);

/// What a window's surface kind, as rinch knows it so far, means for the
/// wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Not known yet: the window does not exist yet.
    Unknown,
    /// A layer surface: as it must be.
    Layer,
    /// An ordinary window: no layer shell here.
    NotLayer,
}

/// The verdict on `kind`.
pub(crate) fn verdict(kind: Option<WindowSurfaceKind>) -> Verdict {
    match kind {
        None => Verdict::Unknown,
        Some(WindowSurfaceKind::LayerShell) => Verdict::Layer,
        Some(WindowSurfaceKind::Toplevel) => Verdict::NotLayer,
    }
}

/// Watches `window` until its surface kind is known; if it is not a layer
/// surface, reports why and ends the app.
pub(crate) fn watch(window: WindowHandle) {
    look(window, 0);
}

/// Looks at `window`'s surface kind once more, `looks` having found
/// nothing.
fn look(window: WindowHandle, looks: u32) {
    set_timeout(LOOK_EVERY_MS, move || {
        if !window.is_open() {
            return;
        }
        match verdict(window.surface_kind()) {
            Verdict::Unknown => match next(looks + 1) {
                Next::LookAgain => look(window, looks + 1),
                Next::GiveUp => tracing::warn!(
                    "wallpaper: rinch never said what a window became, so whether this \
                     compositor has the layer shell is not known; no longer asking (rinch \
                     issue 2659)"
                ),
            },
            Verdict::Layer => {}
            Verdict::NotLayer => refuse(),
        }
    });
}

/// Reports that the compositor has no layer shell, and ends the app.
fn refuse() {
    if !REFUSED.swap(true, Ordering::SeqCst) {
        tracing::error!(
            "wallpaper: this compositor has no layer shell (wlr-layer-shell), so a wallpaper \
             would be an ordinary window over the desktop; not showing one. rinch can say so \
             only once a window has opened (rinch issue 2659)"
        );
    }
    exit();
}

/// Whether the app ended because the compositor has no layer shell: the
/// process then exits with a failure status.
pub(crate) fn refused() -> bool {
    REFUSED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gives_up_after_ten_seconds_of_looks() {
        assert_eq!(LOOKS * LOOK_EVERY_MS, 10_000);
        assert_eq!(next(1), Next::LookAgain);
        assert_eq!(next(LOOKS - 1), Next::LookAgain);
        assert_eq!(next(LOOKS), Next::GiveUp);
        assert_eq!(next(LOOKS + 1), Next::GiveUp);
    }

    #[test]
    fn only_a_layer_surface_will_do() {
        assert_eq!(verdict(None), Verdict::Unknown);
        assert_eq!(verdict(Some(WindowSurfaceKind::LayerShell)), Verdict::Layer);
        assert_eq!(
            verdict(Some(WindowSurfaceKind::Toplevel)),
            Verdict::NotLayer
        );
    }
}
