//! The column's window: one `wlr-layer-shell` surface holding every popup,
//! made when the first notification shows and gone with the last.
//!
//! - In the `overlay` layer, above windows and panels.
//! - Anchored to the top, bottom and right edges, keeping clear of what
//!   other surfaces reserve: its height is the compositor's, the output's
//!   less what panels reserve, and follows it (a panel appearing or going,
//!   the output's mode or scale changing) with no new surface. That height
//!   is the room the popups have. Its width is
//!   [`WIDTH`](crate::policy::WIDTH), fixed. No size is
//!   asked for as popups come and go, so none is ever in flight.
//! - Its margins are none: the space around the popups is the column's own
//!   padding, the theme's spacing.
//! - Transparent, and taking the pointer only over the shown popups
//!   (rinch's `InputRegion::Marked`, each shown popup marked): the padding,
//!   the gaps, the count line and the strip below the column let the
//!   pointer through to what is under them.
//! - Never taking the keyboard: popups are used with the pointer.
//! - On the output the compositor chooses when it is made, kept while any
//!   popup shows.
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules.

use rinch::{LayerAnchor, LayerAxes, LayerKeyboard, LayerShellLayer, LayerSurface};

/// The layer surface's namespace and the window's app id.
pub(crate) const NAMESPACE: &str = "deskrinch-notifications";

/// The window's title.
pub(crate) const TITLE: &str = "deskrinch notifications";

/// The window's height before the compositor gives it its own, which it
/// does in the first configure, before the first frame.
pub(crate) const FIRST_HEIGHT: u32 = 480;

/// The column's layer surface.
pub(crate) fn layer_surface() -> LayerSurface {
    LayerSurface {
        layer: LayerShellLayer::Overlay,
        anchor: LayerAnchor::TOP | LayerAnchor::BOTTOM | LayerAnchor::RIGHT,
        compositor_size: LayerAxes::HEIGHT,
        keyboard: LayerKeyboard::None,
        ..LayerSurface::default()
    }
}

#[cfg(test)]
mod tests {
    use rinch::{LayerExclusiveZone, LayerMargins};

    use super::*;

    #[test]
    fn the_right_edge_of_the_overlay_layer_its_height_the_compositor_s() {
        let surface = layer_surface();
        assert_eq!(surface.layer, LayerShellLayer::Overlay);
        assert_eq!(
            surface.anchor,
            LayerAnchor::TOP | LayerAnchor::BOTTOM | LayerAnchor::RIGHT
        );
        assert_eq!(surface.compositor_size, LayerAxes::HEIGHT);
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::KeepClear);
        assert_eq!(surface.margins, LayerMargins::default());
        assert_eq!(surface.output, None);
        assert_eq!(surface.output_id, None);
    }
}
