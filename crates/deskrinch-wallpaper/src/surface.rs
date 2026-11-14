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
//! # When a window closes
//!
//! The wallpaper never closes its own windows, and assumes the compositor
//! closes a background layer surface only when its output goes. So it
//! treats every close of a window as its output going: nothing to put
//! right and no error, the row (and so the window) going when `outputs()`
//! next changes, and an output that comes back being a new row. That
//! covers an output that goes between rinch listing it and the window for
//! it being made, too: rinch then makes no window (an id no output has is
//! never the compositor's to place elsewhere), reports it closed and logs a
//! warning of its own.
//!
//! What is not handled, as a limit: a compositor that closed the surface
//! while keeping the output would leave that output without a wallpaper
//! until the outputs next change. KWin, sway and labwc close it only with
//! its output.

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

/// Says that the window on the output `output` closed (its `on_closed`):
/// its output went, and its row goes with it (see the module's "When a
/// window closes").
pub(crate) fn closed(output: OutputId) {
    tracing::debug!(
        "wallpaper: the window on output {output:?} closed; its row goes with the output"
    );
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
}
