//! Each output's window: a `wlr-layer-shell` surface on the background
//! layer of that output.
//!
//! - In the `background` layer, below every window and every other layer
//!   surface (the panel is in `top`).
//! - Anchored to all four edges, its size the compositor's on both axes:
//!   the output's, and again when the output's mode, scale or transform
//!   changes, with no new surface.
//! - Ignoring every exclusive zone (`-1`), so it covers what a panel
//!   reserves too; reserving none itself.
//! - Never taking the keyboard, and letting every pointer event through.
//! - Placed on its output by the output's id (`Output::id`), which every
//!   output has, named or not: an output without a name (`wl_output`
//!   before version 4) gets its wallpaper like any other.
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules.
//!
//! An output can go between rinch listing it and the window for it being
//! made. rinch then makes no window (an id no output has is never the
//! compositor's to place elsewhere), reports the window closed and logs a
//! warning of its own; the wallpaper's part is [`closed`]: nothing to put
//! right, no error, and the row goes when `outputs()` next changes.

use rinch::{
    LayerAnchor, LayerAxes, LayerExclusiveZone, LayerKeyboard, LayerShellLayer, LayerSurface,
    OutputId,
};

/// The layer surface's namespace and the windows' app id.
pub(crate) const NAMESPACE: &str = "deskrinch-wallpaper";

/// The windows' title.
pub(crate) const TITLE: &str = "deskrinch wallpaper";

/// The size of a window's first frame, before the compositor assigns its
/// output's.
pub(crate) const FIRST_SIZE: (u32, u32) = (640, 480);

/// The layer surface on the output whose id is `output`.
pub(crate) fn layer_surface(output: OutputId) -> LayerSurface {
    LayerSurface {
        layer: LayerShellLayer::Background,
        anchor: LayerAnchor::ALL,
        exclusive_zone: LayerExclusiveZone::Ignore,
        keyboard: LayerKeyboard::None,
        compositor_size: LayerAxes::BOTH,
        output_id: Some(output),
        ..LayerSurface::default()
    }
}

/// What a window on the output `output` closing means to the wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closed {
    /// Its output went: before the window was made (rinch made none), or
    /// after (the compositor closed the surface). Nothing to put right, and
    /// no error: the output's row, and so its window, goes when `outputs()`
    /// next changes, and an output that comes back is a new row.
    OutputWent(OutputId),
}

/// What a window on the output `output` closing means (its `on_closed`):
/// a window closes for no cause but its output going, the wallpaper never
/// closing its own windows and the compositor closing a background layer
/// surface only when its output goes.
pub(crate) fn closed(output: OutputId) -> Closed {
    let closed = Closed::OutputWent(output);
    tracing::debug!("wallpaper: {closed:?}; its row goes with the output");
    closed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_its_output_on_the_background_layer_and_takes_nothing() {
        let output = OutputId::from_raw(7);
        let surface = layer_surface(output);
        assert_eq!(surface.layer, LayerShellLayer::Background);
        assert_eq!(surface.anchor, LayerAnchor::ALL);
        assert_eq!(surface.compositor_size, LayerAxes::BOTH);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Ignore);
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.margins, rinch::LayerMargins::default());
        // Placed by its id, which every output has, named or not.
        assert_eq!(surface.output_id, Some(output));
        assert_eq!(surface.output, None);
    }

    #[test]
    fn a_window_closed_because_its_output_went_is_nothing_to_put_right() {
        let output = OutputId::from_raw(3);
        assert_eq!(closed(output), Closed::OutputWent(output));
    }
}
