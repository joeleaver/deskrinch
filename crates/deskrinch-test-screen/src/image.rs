//! A captured image and its pixels.

use std::fmt;

/// One pixel: 8-bit sRGB channels with straight (not premultiplied) alpha.
///
/// What an output shows is opaque, so a captured pixel normally has alpha
/// 255; a compositor that leaves part of an output uncovered may report it
/// transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 opaque.
    pub a: u8,
}

impl Rgba {
    /// An opaque colour.
    #[must_use]
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// The colour from premultiplied channels, as Wayland buffers and
    /// KWin's premultiplied formats store them.
    #[must_use]
    pub fn from_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        let straight = |channel: u8| match a {
            0 => 0,
            255 => channel,
            _ => {
                let value = (u32::from(channel) * 255 + u32::from(a) / 2) / u32::from(a);
                u8::try_from(value.min(255)).unwrap_or(u8::MAX)
            }
        };
        Self {
            r: straight(r),
            g: straight(g),
            b: straight(b),
            a,
        }
    }

    /// The colour's premultiplied channels, red first.
    #[must_use]
    pub fn premultiplied(self) -> [u8; 4] {
        let times_alpha = |channel: u8| {
            let value = (u32::from(channel) * u32::from(self.a) + 127) / 255;
            u8::try_from(value).unwrap_or(u8::MAX)
        };
        [
            times_alpha(self.r),
            times_alpha(self.g),
            times_alpha(self.b),
            self.a,
        ]
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{:02x}{:02x}{:02x}{:02x}",
            self.r, self.g, self.b, self.a
        )
    }
}

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
    fn premultiplied_round_trips_where_it_can() {
        let red = Rgba::opaque(200, 10, 0);
        assert_eq!(red.premultiplied(), [200, 10, 0, 255]);
        let [r, g, b, a] = red.premultiplied();
        assert_eq!(Rgba::from_premultiplied(r, g, b, a), red);

        let half = Rgba {
            r: 255,
            g: 0,
            b: 128,
            a: 128,
        };
        assert_eq!(half.premultiplied(), [128, 0, 64, 128]);
        assert_eq!(Rgba::from_premultiplied(128, 0, 64, 128), half);
        assert_eq!(Rgba::from_premultiplied(9, 9, 9, 0), Rgba::default());
    }

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

    #[test]
    fn colours_print_as_hex() {
        assert_eq!(Rgba::opaque(255, 0, 16).to_string(), "#ff0010ff");
    }
}
