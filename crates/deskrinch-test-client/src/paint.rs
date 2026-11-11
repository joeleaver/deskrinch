//! What a test window draws.

use deskrinch_test_wayland::Rgba;

/// What a [`TestWindow`](crate::TestWindow) fills its buffer with, at
/// whatever size the compositor gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paint {
    /// Nothing: fully transparent.
    #[default]
    Transparent,
    /// One colour throughout.
    Solid(Rgba),
    /// Four colours, one per quarter, split at half the width and half the
    /// height (rounded down): a picture that shows a rotated or mirrored
    /// capture for what it is.
    Quadrants {
        /// The top left quarter.
        top_left: Rgba,
        /// The top right quarter.
        top_right: Rgba,
        /// The bottom left quarter.
        bottom_left: Rgba,
        /// The bottom right quarter.
        bottom_right: Rgba,
    },
}

impl Paint {
    /// The colour of the pixel at `(x, y)` in a buffer of `size`.
    #[must_use]
    pub fn color_at(&self, (x, y): (u32, u32), (width, height): (u32, u32)) -> Rgba {
        match *self {
            Self::Transparent => Rgba::default(),
            Self::Solid(color) => color,
            Self::Quadrants {
                top_left,
                top_right,
                bottom_left,
                bottom_right,
            } => match (x < width / 2, y < height / 2) {
                (true, true) => top_left,
                (false, true) => top_right,
                (true, false) => bottom_left,
                (false, false) => bottom_right,
            },
        }
    }

    /// The buffer's bytes in `wl_shm` `argb8888` (premultiplied, blue
    /// first in memory), rows of `width * 4` bytes; `None` when it is all
    /// zeros.
    pub(crate) fn argb8888(&self, (width, height): (u32, u32)) -> Option<Vec<u8>> {
        if *self == Self::Transparent {
            return None;
        }
        let mut bytes = Vec::with_capacity(4 * width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                let [r, g, b, a] = self.color_at((x, y), (width, height)).premultiplied();
                bytes.extend_from_slice(&[b, g, r, a]);
            }
        }
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = Rgba::opaque(255, 0, 0);
    const GREEN: Rgba = Rgba::opaque(0, 255, 0);
    const BLUE: Rgba = Rgba::opaque(0, 0, 255);
    const WHITE: Rgba = Rgba::opaque(255, 255, 255);

    #[test]
    fn quadrants_split_at_the_middle() {
        let paint = Paint::Quadrants {
            top_left: RED,
            top_right: GREEN,
            bottom_left: BLUE,
            bottom_right: WHITE,
        };
        let size = (5, 4);
        assert_eq!(paint.color_at((0, 0), size), RED);
        assert_eq!(paint.color_at((1, 1), size), RED);
        assert_eq!(paint.color_at((2, 1), size), GREEN);
        assert_eq!(paint.color_at((1, 2), size), BLUE);
        assert_eq!(paint.color_at((4, 3), size), WHITE);
    }

    #[test]
    fn writes_argb8888_blue_first() {
        let bytes = Paint::Solid(RED).argb8888((2, 1)).unwrap();
        assert_eq!(bytes, [0, 0, 255, 255, 0, 0, 255, 255]);
        assert_eq!(Paint::Transparent.argb8888((2, 2)), None);
    }
}
