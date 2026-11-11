//! Raw buffers as compositors fill them, turned into an upright [`Image`].

use crate::error::CaptureError;
use crate::image::{Image, Rgba};
use crate::transform::Transform;

/// How a buffer's four bytes per pixel are laid out, in memory order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Blue, green, red, alpha (premultiplied): `wl_shm` `argb8888` and
    /// Qt's `ARGB32_Premultiplied`, little-endian.
    Bgra,
    /// Blue, green, red, unused: `wl_shm` `xrgb8888` and Qt's `RGB32`.
    Bgrx,
    /// Blue, green, red, straight alpha: Qt's `ARGB32`.
    BgraStraight,
    /// Red, green, blue, alpha (premultiplied): `wl_shm` `abgr8888` and
    /// Qt's `RGBA8888_Premultiplied`.
    Rgba,
    /// Red, green, blue, unused: `wl_shm` `xbgr8888` and Qt's `RGBX8888`.
    Rgbx,
    /// Red, green, blue, straight alpha: Qt's `RGBA8888`.
    RgbaStraight,
}

impl Layout {
    fn pixel(self, [b0, b1, b2, b3]: [u8; 4]) -> Rgba {
        match self {
            Self::Bgra => Rgba::from_premultiplied(b2, b1, b0, b3),
            Self::Bgrx => Rgba::opaque(b2, b1, b0),
            Self::BgraStraight => Rgba {
                r: b2,
                g: b1,
                b: b0,
                a: b3,
            },
            Self::Rgba => Rgba::from_premultiplied(b0, b1, b2, b3),
            Self::Rgbx => Rgba::opaque(b0, b1, b2),
            Self::RgbaStraight => Rgba {
                r: b0,
                g: b1,
                b: b2,
                a: b3,
            },
        }
    }
}

/// A filled buffer and what is needed to read it.
#[derive(Debug)]
pub(crate) struct Buffer<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) stride: u32,
    pub(crate) layout: Layout,
    /// Whether the rows run from the bottom up.
    pub(crate) y_inverted: bool,
    /// The transform applied to the picture to make this buffer.
    pub(crate) transform: Transform,
}

impl Buffer<'_> {
    /// The upright image the buffer holds.
    pub(crate) fn to_image(&self) -> Result<Image, CaptureError> {
        let bad = |why: &str| CaptureError::BadBuffer(why.to_owned());
        let row_bytes = u64::from(self.width) * 4;
        if u64::from(self.stride) < row_bytes {
            return Err(bad("its stride is shorter than a row"));
        }
        let needed = u64::from(self.stride) * u64::from(self.height.saturating_sub(1)) + row_bytes;
        if self.height > 0 && (self.bytes.len() as u64) < needed {
            return Err(bad("it is shorter than its size and stride say"));
        }
        let size = if self.transform.swaps_axes() {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let mut pixels = Vec::with_capacity(
            usize::try_from(u64::from(size.0) * u64::from(size.1))
                .map_err(|_| bad("it is too large"))?,
        );
        for y in 0..size.1 {
            for x in 0..size.0 {
                let (bx, by) = self.transform.buffer_position((x, y), size);
                let by = if self.y_inverted {
                    self.height - 1 - by
                } else {
                    by
                };
                let offset = u64::from(by) * u64::from(self.stride) + u64::from(bx) * 4;
                let offset = usize::try_from(offset).map_err(|_| bad("it is too large"))?;
                let bytes = self
                    .bytes
                    .get(offset..offset + 4)
                    .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                    .ok_or_else(|| bad("a pixel lies outside it"))?;
                pixels.push(self.layout.pixel(bytes));
            }
        }
        Image::new(size.0, size.1, pixels).ok_or_else(|| bad("its size overflows"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 buffer, red then blue, in `layout`'s byte order, padded to a
    /// stride of 12.
    fn red_blue(layout: Layout) -> Vec<u8> {
        let (red, blue): ([u8; 4], [u8; 4]) = match layout {
            Layout::Bgra | Layout::BgraStraight | Layout::Bgrx => {
                ([0, 0, 255, 255], [255, 0, 0, 255])
            }
            Layout::Rgba | Layout::RgbaStraight | Layout::Rgbx => {
                ([255, 0, 0, 255], [0, 0, 255, 255])
            }
        };
        [&red[..], &blue[..], &[9; 4][..]].concat()
    }

    fn buffer(bytes: &[u8], layout: Layout, transform: Transform, y_inverted: bool) -> Buffer<'_> {
        Buffer {
            bytes,
            width: 2,
            height: 1,
            stride: 12,
            layout,
            y_inverted,
            transform,
        }
    }

    const RED: Rgba = Rgba::opaque(255, 0, 0);
    const BLUE: Rgba = Rgba::opaque(0, 0, 255);

    #[test]
    fn each_layout_reads_red_and_blue() {
        for layout in [
            Layout::Bgra,
            Layout::Bgrx,
            Layout::BgraStraight,
            Layout::Rgba,
            Layout::Rgbx,
            Layout::RgbaStraight,
        ] {
            let bytes = red_blue(layout);
            let image = buffer(&bytes, layout, Transform::Normal, false)
                .to_image()
                .unwrap();
            assert_eq!(image.pixels(), [RED, BLUE], "{layout:?}");
        }
    }

    #[test]
    fn unused_alpha_reads_as_opaque() {
        let bytes = [10, 20, 30, 0];
        let image = Buffer {
            bytes: &bytes,
            width: 1,
            height: 1,
            stride: 4,
            layout: Layout::Bgrx,
            y_inverted: false,
            transform: Transform::Normal,
        }
        .to_image()
        .unwrap();
        assert_eq!(image.pixels(), [Rgba::opaque(30, 20, 10)]);
    }

    #[test]
    fn a_quarter_turn_is_undone() {
        let bytes = red_blue(Layout::Rgba);
        // The buffer is the picture turned a quarter counter-clockwise: the
        // top of the picture's one column went to the left.
        let image = buffer(&bytes, Layout::Rgba, Transform::Rotated90, false)
            .to_image()
            .unwrap();
        assert_eq!((image.width(), image.height()), (1, 2));
        assert_eq!(image.pixels(), [RED, BLUE]);
        // A three-quarter turn sent it to the right.
        let image = buffer(&bytes, Layout::Rgba, Transform::Rotated270, false)
            .to_image()
            .unwrap();
        assert_eq!(image.pixels(), [BLUE, RED]);
    }

    #[test]
    fn rows_from_the_bottom_are_put_right() {
        let bytes = [[0, 0, 255, 255], [255, 0, 0, 255]].concat();
        let image = Buffer {
            bytes: &bytes,
            width: 1,
            height: 2,
            stride: 4,
            layout: Layout::Bgra,
            y_inverted: true,
            transform: Transform::Normal,
        }
        .to_image()
        .unwrap();
        assert_eq!(image.pixels(), [BLUE, RED]);
    }

    #[test]
    fn a_short_buffer_is_refused() {
        let bytes = [0; 7];
        let short = Buffer {
            bytes: &bytes,
            width: 2,
            height: 1,
            stride: 8,
            layout: Layout::Bgra,
            y_inverted: false,
            transform: Transform::Normal,
        };
        assert!(matches!(short.to_image(), Err(CaptureError::BadBuffer(_))));
        let narrow = Buffer { stride: 4, ..short };
        assert!(matches!(narrow.to_image(), Err(CaptureError::BadBuffer(_))));
    }
}
