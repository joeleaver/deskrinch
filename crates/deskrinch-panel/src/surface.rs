//! The panel's window: a `wlr-layer-shell` surface along the bottom edge of
//! the output.
//!
//! - In the `top` layer, above ordinary windows.
//! - Anchored to the bottom, left and right edges, with its width assigned
//!   by the compositor: the output's width, and again when the output's
//!   mode changes.
//! - [`HEIGHT`] logical pixels high, and reserving that much as its
//!   exclusive zone, so maximised windows stop at its top edge.
//! - Never taking the keyboard: clicking it leaves the focus where it was.
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules (`layerrule` in Hyprland, KWin's
//!   window rules).
//!
//! The output is the compositor's choice. Which edge, how high and which
//! outputs become settings when the configuration crate lands.

use rinch::{
    LayerAnchor, LayerAxes, LayerExclusiveZone, LayerKeyboard, LayerShellLayer, LayerSurface,
    WindowProps,
};

/// The panel's height, logical pixels.
pub(crate) const HEIGHT: u32 = 40;

/// The layer surface's namespace and the window's app id.
pub(crate) const NAMESPACE: &str = "deskrinch-panel";

/// The width of the first frame, before the compositor assigns the
/// output's (and the window's on a compositor without the layer shell).
const FIRST_WIDTH: u32 = 800;

/// The panel's layer surface.
pub(crate) fn layer_surface() -> LayerSurface {
    LayerSurface {
        layer: LayerShellLayer::Top,
        anchor: LayerAnchor::BOTTOM | LayerAnchor::LEFT | LayerAnchor::RIGHT,
        exclusive_zone: LayerExclusiveZone::Reserve(HEIGHT),
        keyboard: LayerKeyboard::None,
        compositor_size: LayerAxes::WIDTH,
        ..LayerSurface::default()
    }
}

/// The panel's window.
pub(crate) fn window_props() -> WindowProps {
    WindowProps {
        title: "deskrinch panel".into(),
        width: FIRST_WIDTH,
        height: HEIGHT,
        borderless: true,
        app_id: Some(NAMESPACE.into()),
        wayland_layer: Some(layer_surface()),
        ..WindowProps::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_the_bottom_edge_and_reserves_its_height() {
        let surface = layer_surface();
        assert_eq!(surface.layer, LayerShellLayer::Top);
        assert_eq!(
            surface.anchor,
            LayerAnchor::BOTTOM | LayerAnchor::LEFT | LayerAnchor::RIGHT
        );
        assert_eq!(surface.compositor_size, LayerAxes::WIDTH);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Reserve(HEIGHT));
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.output, None);
        assert_eq!(surface.margins, rinch::LayerMargins::default());
    }

    #[test]
    fn is_a_borderless_window_of_its_height_named_for_the_compositor() {
        let props = window_props();
        assert_eq!(props.height, HEIGHT);
        assert!(props.borderless);
        assert_eq!(props.app_id.as_deref(), Some("deskrinch-panel"));
        assert_eq!(props.wayland_layer, Some(layer_surface()));
    }
}
