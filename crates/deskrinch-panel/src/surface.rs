//! The panel's window: a `wlr-layer-shell` surface along one edge of the
//! output, placed by the [`Placement`] its settings give.
//!
//! - In the `top` layer, above ordinary windows.
//! - Anchored to its edge (bottom or top) and to the left and right edges,
//!   with its width assigned by the compositor: the output's width, and
//!   again when the output's mode changes.
//! - Its height high, and reserving that much of its edge as its exclusive
//!   zone, so maximised windows stop short of it.
//! - Never taking the keyboard: clicking it leaves the focus where it was.
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules (`layerrule` in Hyprland, KWin's
//!   window rules).
//!
//! The output is the compositor's choice.
//!
//! [`place`] moves a running panel: the window's new height, then the new
//! anchors and zone. Both are requests queued to rinch's event loop from
//! the main thread in one go, so rinch applies them in the same turn and
//! the compositor sees them in one commit: rinch's `set_current_window_size`
//! only stages a layer surface's size, and `set_current_window_layer`
//! sends every changed field with it and commits once.

use rinch::windows::{current_window_size, set_current_window_layer, set_current_window_size};
use rinch::{
    LayerAnchor, LayerAxes, LayerExclusiveZone, LayerKeyboard, LayerShellLayer, LayerSurface,
    WindowProps,
};

use crate::settings::{Edge, Placement};

/// The layer surface's namespace and the window's app id.
pub(crate) const NAMESPACE: &str = "deskrinch-panel";

/// The width of the first frame, before the compositor assigns the
/// output's (and the window's on a compositor without the layer shell).
const FIRST_WIDTH: u32 = 800;

/// The panel's layer surface at `placement`.
pub(crate) fn layer_surface(placement: Placement) -> LayerSurface {
    let edge = match placement.edge {
        Edge::Bottom => LayerAnchor::BOTTOM,
        Edge::Top => LayerAnchor::TOP,
    };
    LayerSurface {
        layer: LayerShellLayer::Top,
        anchor: edge | LayerAnchor::LEFT | LayerAnchor::RIGHT,
        exclusive_zone: LayerExclusiveZone::Reserve(placement.height.get()),
        keyboard: LayerKeyboard::None,
        compositor_size: LayerAxes::WIDTH,
        ..LayerSurface::default()
    }
}

/// The panel's window, made at `placement`.
pub(crate) fn window_props(placement: Placement) -> WindowProps {
    WindowProps {
        title: "deskrinch panel".into(),
        width: FIRST_WIDTH,
        height: placement.height.get(),
        borderless: true,
        app_id: Some(NAMESPACE.into()),
        wayland_layer: Some(layer_surface(placement)),
        ..WindowProps::default()
    }
}

/// Moves the running panel to `placement`. Called on the main thread.
pub(crate) fn place(placement: Placement) {
    // The width is the compositor's on a layer surface whatever is asked;
    // the current one is asked so that a compositor without the layer
    // shell keeps it too.
    let width = current_window_size().map_or(f64::from(FIRST_WIDTH), |(width, _)| width);
    set_current_window_size(width, f64::from(placement.height.get()));
    set_current_window_layer(layer_surface(placement));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Height, PanelSettings};

    fn placement(edge: Edge, height: u32) -> Placement {
        Placement {
            edge,
            height: Height::try_from(height).unwrap(),
        }
    }

    #[test]
    fn by_default_spans_the_bottom_edge_and_reserves_40() {
        let surface = layer_surface(PanelSettings::default().placement());
        assert_eq!(surface.layer, LayerShellLayer::Top);
        assert_eq!(
            surface.anchor,
            LayerAnchor::BOTTOM | LayerAnchor::LEFT | LayerAnchor::RIGHT
        );
        assert_eq!(surface.compositor_size, LayerAxes::WIDTH);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Reserve(40));
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.output, None);
        assert_eq!(surface.margins, rinch::LayerMargins::default());
    }

    #[test]
    fn at_the_top_anchors_and_reserves_the_top_edge() {
        let surface = layer_surface(placement(Edge::Top, 56));
        assert_eq!(
            surface.anchor,
            LayerAnchor::TOP | LayerAnchor::LEFT | LayerAnchor::RIGHT
        );
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Reserve(56));
        assert_eq!(surface.compositor_size, LayerAxes::WIDTH);
    }

    #[test]
    fn is_a_borderless_window_of_its_height_named_for_the_compositor() {
        let at = placement(Edge::Bottom, 24);
        let props = window_props(at);
        assert_eq!(props.height, 24);
        assert!(props.borderless);
        assert_eq!(props.app_id.as_deref(), Some("deskrinch-panel"));
        assert_eq!(props.wayland_layer, Some(layer_surface(at)));
    }
}
