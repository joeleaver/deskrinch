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
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules.

use rinch::prelude::Output;
use rinch::{
    LayerAnchor, LayerAxes, LayerExclusiveZone, LayerKeyboard, LayerShellLayer, LayerSurface,
};

/// The layer surface's namespace and the windows' app id.
pub(crate) const NAMESPACE: &str = "deskrinch-wallpaper";

/// The windows' title, which compositors without the layer shell show.
pub(crate) const TITLE: &str = "deskrinch wallpaper";

/// The size of a window's first frame, before the compositor assigns its
/// output's.
pub(crate) const FIRST_SIZE: (u32, u32) = (640, 480);

/// The layer surface on the output named `output`.
pub(crate) fn layer_surface(output: String) -> LayerSurface {
    LayerSurface {
        layer: LayerShellLayer::Background,
        anchor: LayerAnchor::ALL,
        exclusive_zone: LayerExclusiveZone::Ignore,
        keyboard: LayerKeyboard::None,
        compositor_size: LayerAxes::BOTH,
        output: Some(output),
        ..LayerSurface::default()
    }
}

/// The outputs that get a window, by the name that keys it and places its
/// layer surface, and how many outputs get none because they have no name.
///
/// An output has no name with `wl_output` before version 4, and then
/// neither keys a window of its own nor can have a layer surface placed on
/// it; it gets no window (rinch issue 2658, which gives every output an id
/// to key and place by; this goes when it lands). Every compositor
/// deskrinch runs on names its outputs.
pub(crate) fn placeable(outputs: Vec<Output>) -> (Vec<String>, usize) {
    let mut unnamed = 0;
    let names = outputs
        .into_iter()
        .filter_map(|output| {
            if output.name.is_none() {
                unnamed += 1;
            }
            output.name
        })
        .collect();
    (names, unnamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_its_output_on_the_background_layer_and_takes_nothing() {
        let surface = layer_surface("DP-1".into());
        assert_eq!(surface.layer, LayerShellLayer::Background);
        assert_eq!(surface.anchor, LayerAnchor::ALL);
        assert_eq!(surface.compositor_size, LayerAxes::BOTH);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::Ignore);
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.output.as_deref(), Some("DP-1"));
        assert_eq!(surface.margins, rinch::LayerMargins::default());
    }

    fn output(name: Option<&str>) -> Output {
        Output {
            name: name.map(ToOwned::to_owned),
            position: (0.0, 0.0),
            size: (1280.0, 720.0),
            scale: 1.0,
        }
    }

    #[test]
    fn a_window_per_named_output_and_none_for_one_without_a_name() {
        let (names, unnamed) =
            placeable(vec![output(None), output(Some("HDMI-A-1")), output(None)]);
        // The named one keeps its own window; the two unnamed ones get
        // none, and share no key.
        assert_eq!(names, ["HDMI-A-1"]);
        assert_eq!(unnamed, 2);
        assert_eq!(placeable(Vec::new()), (Vec::new(), 0));
    }
}
