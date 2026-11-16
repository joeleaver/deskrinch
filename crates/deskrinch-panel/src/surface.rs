//! Each bar's window: a `wlr-layer-shell` surface along one edge of its
//! output, placed by the [`Placement`] the settings give.
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
//! - Placed on its output by the output's id (`Output::id`), which every
//!   output has, named or not.
//!
//! # When a window closes
//!
//! The panel never closes its own windows, and the compositor closes a
//! layer surface only when its output goes: every close is treated as the
//! output going ([`closed`]), the bar's row goes when the output leaves
//! rinch's `output_ids()`, and an output that comes back is a new row with a
//! new window. As for the wallpaper, a compositor that closed the surface
//! while keeping the output would leave that output without a bar until it
//! is turned off and on again.
//!
//! [`place`] moves a running bar: the window's new height and its new
//! anchors and zone, asked for together from one effect on the main thread,
//! inside the bar's window, which is then rinch's current window.
//! rinch guarantees that on a layer surface what one turn of its event loop
//! asks through `set_current_window_size`, `set_current_window_layer` and
//! `set_current_window_position` reaches the compositor in one commit,
//! whatever the order of the calls (joeleaver/rinch#2464, documented on
//! those functions). So the compositor arranges the moved panel once, never
//! at its new height with its old zone.

use rinch::windows::{current_window_size, set_current_window_layer, set_current_window_size};
use rinch::{
    LayerAnchor, LayerAxes, LayerExclusiveZone, LayerKeyboard, LayerShellLayer, LayerSurface,
    OutputId,
};

use deskrinch_settings::panel::{Edge, Height, PanelSettings};

/// Where the panel is: the part of its settings its window's surface is
/// made of. A change to it moves the window; any other change does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Placement {
    /// The edge it runs along.
    pub(crate) edge: Edge,
    /// Its height, which it also reserves.
    pub(crate) height: Height,
}

impl Placement {
    /// Where `settings` put the panel.
    pub(crate) fn of(settings: &PanelSettings) -> Self {
        Self {
            edge: settings.edge,
            height: settings.height,
        }
    }
}

/// The layer surface's namespace and the windows' app id.
pub(crate) const NAMESPACE: &str = "deskrinch-panel";

/// The windows' title.
pub(crate) const TITLE: &str = "deskrinch panel";

/// The width of the first frame, before the compositor assigns the
/// output's (and the window's on a compositor without the layer shell).
pub(crate) const FIRST_WIDTH: u32 = 800;

/// The layer surface of the bar on the output whose id is `output`, at
/// `placement`.
pub(crate) fn layer_surface(placement: Placement, output: OutputId) -> LayerSurface {
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
        output_id: Some(output),
        ..LayerSurface::default()
    }
}

/// Moves the running bar on `output` to `placement`. Called on the main
/// thread, from one effect inside the bar's window (rinch's current window
/// there), so that both requests fall in one turn and one commit.
pub(crate) fn place(placement: Placement, output: OutputId) {
    // The width is the compositor's on a layer surface whatever is asked;
    // the current one is asked so that a compositor without the layer
    // shell keeps it too.
    let width = current_window_size().map_or(f64::from(FIRST_WIDTH), |(width, _)| width);
    set_current_window_size(width, f64::from(placement.height.get()));
    set_current_window_layer(layer_surface(placement, output));
    tracing::info!(
        "{}",
        deskrinch_panel::placed(placement.edge, placement.height)
    );
}

/// Says that the bar's window on the output `output` closed (its
/// `on_closed`): its output went, and its row goes with it (see the
/// module's "When a window closes").
pub(crate) fn closed(output: OutputId) {
    tracing::debug!("panel: the bar on output {output:?} closed; its row goes with the output");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(edge: Edge, height: u32) -> Placement {
        Placement {
            edge,
            height: Height::try_from(height).unwrap(),
        }
    }

    /// An output's id, as rinch gives one.
    const OUTPUT: OutputId = OutputId::from_raw(7);

    #[test]
    fn by_default_spans_the_bottom_edge_and_reserves_40() {
        let surface = layer_surface(Placement::of(&PanelSettings::default()), OUTPUT);
        assert_eq!(surface.layer, LayerShellLayer::Top);
        assert_eq!(
            surface.anchor,
            LayerAnchor::BOTTOM | LayerAnchor::LEFT | LayerAnchor::RIGHT
        );
        assert_eq!(surface.compositor_size, LayerAxes::WIDTH);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Reserve(40));
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.margins, rinch::LayerMargins::default());
    }

    #[test]
    fn is_placed_on_its_output_by_id() {
        let surface = layer_surface(Placement::of(&PanelSettings::default()), OUTPUT);
        // By its id, which every output has, named or not.
        assert_eq!(surface.output_id, Some(OUTPUT));
        assert_eq!(surface.output, None);
    }

    #[test]
    fn at_the_top_anchors_and_reserves_the_top_edge() {
        let surface = layer_surface(placement(Edge::Top, 56), OUTPUT);
        assert_eq!(
            surface.anchor,
            LayerAnchor::TOP | LayerAnchor::LEFT | LayerAnchor::RIGHT
        );
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Reserve(56));
        assert_eq!(surface.compositor_size, LayerAxes::WIDTH);
    }
}
