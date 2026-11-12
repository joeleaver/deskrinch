//! The launcher's window: a `wlr-layer-shell` surface in the overlay layer,
//! centred on the output, taking all keyboard input while it is shown.
//!
//! - In the `overlay` layer, above ordinary windows and panels.
//! - Anchored to no edge: the compositor centres it on its output, which is
//!   the compositor's choice.
//! - Keyboard interactivity `exclusive`: while it is mapped every key goes
//!   to it. Hidden, rinch unmaps it (its surface is destroyed and made again
//!   at the next show), and the compositor gives the keyboard back to the
//!   window that had it.
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules.
//! - A fixed size, [`WIDTH`] by [`HEIGHT`]: nothing in the configuration
//!   sets it.

use rinch::{LayerAnchor, LayerKeyboard, LayerShellLayer, LayerSurface, WindowProps};

/// The layer surface's namespace and the window's app id.
pub(crate) const NAMESPACE: &str = "deskrinch-launcher";

/// The window's width, in logical pixels.
pub(crate) const WIDTH: u32 = 640;
/// The window's height, in logical pixels.
pub(crate) const HEIGHT: u32 = 420;

/// The launcher's layer surface.
fn layer_surface() -> LayerSurface {
    LayerSurface {
        layer: LayerShellLayer::Overlay,
        anchor: LayerAnchor::NONE,
        keyboard: LayerKeyboard::Exclusive,
        ..LayerSurface::default()
    }
}

/// The launcher's window, made `shown` or hidden.
pub(crate) fn window_props(shown: bool) -> WindowProps {
    WindowProps {
        title: "deskrinch launcher".into(),
        width: WIDTH,
        height: HEIGHT,
        borderless: true,
        visible: shown,
        app_id: Some(NAMESPACE.into()),
        wayland_layer: Some(layer_surface()),
        ..WindowProps::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_a_centred_overlay_with_the_keyboard() {
        let props = window_props(false);
        assert!(!props.visible);
        assert!(window_props(true).visible);
        let surface = props.wayland_layer.unwrap();
        assert_eq!(surface.layer, LayerShellLayer::Overlay);
        assert_eq!(surface.anchor, LayerAnchor::NONE);
        assert_eq!(surface.keyboard, LayerKeyboard::Exclusive);
        assert_eq!(props.app_id.as_deref(), Some(NAMESPACE));
        assert_eq!((props.width, props.height), (WIDTH, HEIGHT));
    }
}
