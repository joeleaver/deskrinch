//! A captured image and its pixels.

use std::fmt;

use deskrinch_test_wayland::Rgba;

/// What an output showed: its pixels, upright and in physical pixels.
///
/// - **Size**: the output's current mode, in physical pixels, with width and
///   height swapped for an output rotated a quarter turn. An output's scale
///   does not change it: a 1280×720 output at scale 2 gives a 1280×720
///   image, in which a client's 100×100 surface covers 200×200 pixels.
/// - **Orientation**: upright, as someone looking at the screen sees it:
///   the output's transform (rotation and flip) has been undone, and so has
///   a buffer the compositor filled upside down. The top-left pixel is the
///   top-left corner of the output's area in the compositor's layout.
/// - **Pixels**: [`Rgba`], row-major, rows from top to bottom, each row left
///   to right, with no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<Rgba>,
}

impl Image {
    /// An image of `width` × `height` from its pixels, row-major from the
    /// top; `None` when the count does not match the size.
    #[must_use]
    pub fn new(width: u32, height: u32, pixels: Vec<Rgba>) -> Option<Self> {
        let count = usize::try_from(u64::from(width) * u64::from(height)).ok()?;
        (pixels.len() == count).then_some(Self {
            width,
            height,
            pixels,
        })
    }

    /// Its width in pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Its height in pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixel at column `x` and row `y` (from the top left), or `None`
    /// outside the image.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = usize::try_from(u64::from(y) * u64::from(self.width) + u64::from(x)).ok()?;
        self.pixels.get(index).copied()
    }

    /// Every pixel, row-major from the top left.
    #[must_use]
    pub fn pixels(&self) -> &[Rgba] {
        &self.pixels
    }

    /// Row `y` (from the top), or `None` outside the image.
    #[must_use]
    pub fn row(&self, y: u32) -> Option<&[Rgba]> {
        let width = usize::try_from(self.width).ok()?;
        let y = usize::try_from(y).ok()?;
        self.pixels.chunks_exact(width.max(1)).nth(y)
    }
}

impl fmt::Debug for Image {
    /// The size and the first pixel only: an image has millions of pixels.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("top_left", &self.pixels.first())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_row_major_from_the_top() {
        let pixels = (0..6).map(|i| Rgba::opaque(i, 0, 0)).collect();
        let image = Image::new(3, 2, pixels).unwrap();
        assert_eq!(image.pixel(0, 0), Some(Rgba::opaque(0, 0, 0)));
        assert_eq!(image.pixel(2, 0), Some(Rgba::opaque(2, 0, 0)));
        assert_eq!(image.pixel(0, 1), Some(Rgba::opaque(3, 0, 0)));
        assert_eq!(image.pixel(3, 0), None);
        assert_eq!(image.pixel(0, 2), None);
        assert_eq!(image.row(1).unwrap().len(), 3);
        assert_eq!(image.row(1).unwrap()[2], Rgba::opaque(5, 0, 0));
        assert_eq!(image.row(2), None);
    }

    #[test]
    fn the_pixel_count_must_match() {
        assert!(Image::new(2, 2, vec![Rgba::default(); 3]).is_none());
        assert!(Image::new(0, 5, Vec::new()).is_some());
    }
}
