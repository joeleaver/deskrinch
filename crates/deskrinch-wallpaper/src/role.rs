//! Whether the wallpaper's windows are what a wallpaper must be: layer
//! surfaces on the background layer. On a compositor without the layer
//! shell (GNOME), rinch makes each an ordinary window instead, which would
//! cover the desktop; the wallpaper then says why and exits, with a failure
//! status, opening no more.
//!
//! rinch says what a window became only once it exists (rinch issue 2659:
//! nothing answers before a window opens). So each window, as it opens,
//! looks at its surface kind until rinch knows it: a few turns of the event
//! loop at the start, and never again.

use std::sync::atomic::{AtomicBool, Ordering};

use rinch::prelude::{WindowHandle, WindowSurfaceKind, exit, set_timeout};

/// How often a window looks at its surface kind until rinch knows it.
const LOOK_EVERY_MS: u32 = 50;

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
    set_timeout(LOOK_EVERY_MS, move || {
        if !window.is_open() {
            return;
        }
        match verdict(window.surface_kind()) {
            Verdict::Unknown => watch(window),
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
    fn only_a_layer_surface_will_do() {
        assert_eq!(verdict(None), Verdict::Unknown);
        assert_eq!(verdict(Some(WindowSurfaceKind::LayerShell)), Verdict::Layer);
        assert_eq!(
            verdict(Some(WindowSurfaceKind::Toplevel)),
            Verdict::NotLayer
        );
    }
}
