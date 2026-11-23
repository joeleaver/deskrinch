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
//! A bar's [`BarSurface`] (its height and layer surface, one value) is a
//! pure function of the settings' placement and its output. Its window is
//! opened with it, and [`place`] moves a running bar to another: the new
//! height and the new anchors and zone, asked for together through the
//! window's handle, from one effect on the main thread. rinch guarantees
//! that on a layer surface what one turn of its event loop asks of a
//! window, its size and its layer, reaches the compositor in one commit,
//! whatever the order of the calls (joeleaver/rinch#2464). So the
//! compositor arranges the moved bar once, never at its new height with
//! its old zone. A handle's requests are addressed to its window and queued
//! behind its opening, so a request made before the window exists reaches
//! it once it does.

use rinch::prelude::untracked;
use rinch::{
    LayerAnchor, LayerAxes, LayerExclusiveZone, LayerKeyboard, LayerShellLayer, LayerSurface,
    OutputId, WindowHandle,
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

/// What a bar's window is: its height and its layer surface, as one value,
/// so that a change of either is one change.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarSurface {
    /// Its height, which it also reserves.
    pub(crate) height: u32,
    /// Its layer surface.
    pub(crate) layer: LayerSurface,
}

/// The window of the bar on the output whose id is `output`, at
/// `placement`.
pub(crate) fn bar_surface(placement: Placement, output: OutputId) -> BarSurface {
    BarSurface {
        height: placement.height.get(),
        layer: layer_surface(placement, output),
    }
}

/// The layer surface of the bar on the output whose id is `output`, at
/// `placement`.
fn layer_surface(placement: Placement, output: OutputId) -> LayerSurface {
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

/// What to ask of a bar's window, if anything: `wanted`, when it differs
/// from what the window was opened with or last asked for (`applied`), and
/// only while the bar's output is `connected`. A bar acts only while its
/// output exists: once it has gone, its window is closing, and anything
/// asked of it would be asked of a window that no longer is.
pub(crate) fn to_send(
    applied: &BarSurface,
    wanted: BarSurface,
    connected: bool,
) -> Option<BarSurface> {
    (connected && wanted != *applied).then_some(wanted)
}

/// Moves the bar on `output`, whose window is `window`, to `surface`, at
/// `placement`. Called on the main thread, from one effect, so that both
/// requests fall in one turn and one commit.
pub(crate) fn place(
    window: WindowHandle,
    output: OutputId,
    surface: &BarSurface,
    placement: Placement,
) {
    // The width is the compositor's on a layer surface whatever is asked
    // (`compositor_size`); on a compositor without the layer shell the
    // window is an ordinary one, which keeps its width. Before the window
    // exists its handle answers no size, and the width it opens with is
    // the first.
    let width = window
        .size()
        .map_or(f64::from(FIRST_WIDTH), |(width, _)| width);
    window.set_size(width, f64::from(surface.height));
    window.set_layer(surface.layer.clone());
    tracing::info!(
        "{}",
        deskrinch_panel::placed(&output_name(output), placement.edge, placement.height)
    );
}

/// The output's name, or its id where it has none, for a line about its
/// bar. Read untracked: a bar's effect follows its placement, not the
/// outputs.
fn output_name(output: OutputId) -> String {
    untracked(|| rinch::prelude::output(output))
        .and_then(|output| output.name)
        .unwrap_or_else(|| format!("{output:?}"))
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

    /// A bar's window is asked for its surface only when it differs from
    /// what it was opened with or last asked for: the edge, the height, or
    /// both together, as one request.
    #[test]
    fn a_window_is_asked_only_for_a_surface_it_does_not_have() {
        let opened = bar_surface(placement(Edge::Bottom, 40), OUTPUT);
        assert_eq!(
            to_send(
                &opened,
                bar_surface(placement(Edge::Bottom, 40), OUTPUT),
                true
            ),
            None
        );
        let moved = bar_surface(placement(Edge::Top, 40), OUTPUT);
        assert_eq!(to_send(&opened, moved.clone(), true), Some(moved.clone()));
        let taller = bar_surface(placement(Edge::Bottom, 56), OUTPUT);
        assert_eq!(to_send(&opened, taller.clone(), true), Some(taller));
        let both = bar_surface(placement(Edge::Top, 56), OUTPUT);
        assert_eq!(to_send(&moved, both.clone(), true), Some(both));
        // Another output's bar is another surface.
        let elsewhere = bar_surface(placement(Edge::Bottom, 40), OutputId::from_raw(8));
        assert_eq!(to_send(&opened, elsewhere.clone(), true), Some(elsewhere));
    }

    #[test]
    fn a_bar_whose_output_has_gone_asks_nothing() {
        let opened = bar_surface(placement(Edge::Bottom, 40), OUTPUT);
        let moved = bar_surface(placement(Edge::Top, 56), OUTPUT);
        assert_eq!(to_send(&opened, moved.clone(), true), Some(moved.clone()));
        assert_eq!(to_send(&opened, moved, false), None);
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
