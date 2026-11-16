//! The column's window: one `wlr-layer-shell` surface holding every popup,
//! made when the first notification shows and gone with the last.
//!
//! - In the `overlay` layer, above windows and panels.
//! - Anchored to the top and right edges, keeping clear of what other
//!   surfaces reserve (a panel along the top pushes it down). Its margins
//!   are none: the space around the popups is the column's own padding,
//!   the theme's spacing.
//! - Sized by its content (rinch's `size_to_content`), at most
//!   [`MAX_WIDTH`](crate::policy::MAX_WIDTH) wide and the output high; a
//!   column taller than that scrolls.
//! - Transparent: nothing paints but each popup its own background. Every
//!   pixel of the surface takes the pointer, the gaps between popups and
//!   the padding around them included, until rinch can give a surface an
//!   input region; then the window takes only its painted pixels, with no
//!   other change.
//! - Never taking the keyboard: popups are used with the pointer.
//! - On the output the compositor chooses when it is made, kept while any
//!   popup shows.
//! - Its namespace, [`NAMESPACE`], is the window's app id, which is how
//!   compositors name it in their rules.

use rinch::{ContentSizing, LayerAnchor, LayerKeyboard, LayerShellLayer, LayerSurface};

use crate::policy::MAX_WIDTH;

/// The layer surface's namespace and the window's app id.
pub(crate) const NAMESPACE: &str = "deskrinch-notifications";

/// The window's title.
pub(crate) const TITLE: &str = "deskrinch notifications";

/// The column's layer surface.
pub(crate) fn layer_surface() -> LayerSurface {
    LayerSurface {
        layer: LayerShellLayer::Overlay,
        anchor: LayerAnchor::TOP | LayerAnchor::RIGHT,
        keyboard: LayerKeyboard::None,
        ..LayerSurface::default()
    }
}

/// How the column is sized: by its content, at most [`MAX_WIDTH`] wide.
pub(crate) fn sizing() -> ContentSizing {
    ContentSizing::new().with_max_width(MAX_WIDTH)
}

#[cfg(test)]
mod tests {
    use rinch::{LayerAxes, LayerExclusiveZone, LayerMargins};

    use super::*;

    #[test]
    fn top_right_on_the_overlay_layer_taking_no_keyboard() {
        let surface = layer_surface();
        assert_eq!(surface.layer, LayerShellLayer::Overlay);
        assert_eq!(surface.anchor, LayerAnchor::TOP | LayerAnchor::RIGHT);
        assert_eq!(surface.keyboard, LayerKeyboard::None);
        assert_eq!(surface.exclusive_zone, LayerExclusiveZone::KeepClear);
        assert_eq!(surface.margins, LayerMargins::default());
        assert_eq!(surface.compositor_size, LayerAxes::default());
        assert_eq!(surface.output, None);
        assert_eq!(surface.output_id, None);
    }

    #[test]
    fn sized_by_its_content_within_the_width() {
        let sizing = sizing();
        assert_eq!(sizing.max_width, Some(MAX_WIDTH));
        assert_eq!(sizing.max_height, None);
    }
}
