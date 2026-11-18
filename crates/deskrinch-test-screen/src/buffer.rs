//! Raw buffers as compositors fill them, turned into an upright [`Image`].

use crate::error::CaptureError;
use crate::image::Image;
use crate::{Rgba, Transform};

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
    /// How a pixel's four bytes, in memory order, read in this layout:
    /// chosen once per buffer, not matched per pixel.
    fn reader(self) -> fn(&[u8]) -> Rgba {
        match self {
            Self::Bgra => |p| Rgba::from_premultiplied(p[2], p[1], p[0], p[3]),
            Self::Bgrx => |p| Rgba::opaque(p[2], p[1], p[0]),
            Self::BgraStraight => |p| Rgba {
                r: p[2],
                g: p[1],
                b: p[0],
                a: p[3],
            },
            Self::Rgba => |p| Rgba::from_premultiplied(p[0], p[1], p[2], p[3]),
            Self::Rgbx => |p| Rgba::opaque(p[0], p[1], p[2]),
            Self::RgbaStraight => |p| Rgba {
                r: p[0],
                g: p[1],
                b: p[2],
                a: p[3],
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
    ///
    /// Row by row: an upright row lies along one of the buffer's rows or
    /// columns, whatever the transform, so it is the pixel its first lies at
    /// and a fixed step to the next. Where the step is a pixel forwards (no
    /// quarter turn and no mirror) the row is one run of the buffer's bytes.
    /// Each row's two ends are checked against the buffer once, which
    /// bounds every pixel between them.
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
        let too_large = || bad("it is too large");
        let mut pixels = Vec::with_capacity(
            usize::try_from(u64::from(size.0) * u64::from(size.1)).map_err(|_| too_large())?,
        );
        let read = self.layout.reader();
        let width = usize::try_from(size.0).map_err(|_| too_large())?;
        for y in 0..size.1 {
            if width == 0 {
                break;
            }
            let first = self.offset((0, y), size);
            let step = if width > 1 {
                self.offset((1, y), size) - first
            } else {
                4
            };
            let last = first + step * (i64::from(size.0) - 1);
            let inside = |offset: i64| {
                offset >= 0 && u64::try_from(offset).is_ok_and(|o| o + 4 <= self.bytes.len() as u64)
            };
            if !inside(first) || !inside(last) {
                return Err(bad("a pixel lies outside it"));
            }
            let first = usize::try_from(first).map_err(|_| too_large())?;
            if step == 4 {
                let row = &self.bytes[first..first + width * 4];
                pixels.extend(row.as_chunks::<4>().0.iter().map(|pixel| read(pixel)));
            } else {
                let step = isize::try_from(step).map_err(|_| too_large())?;
                let mut at = first;
                for _ in 0..width {
                    pixels.push(read(&self.bytes[at..at + 4]));
                    at = at.wrapping_add_signed(step);
                }
            }
        }
        Image::new(size.0, size.1, pixels).ok_or_else(|| bad("its size overflows"))
    }

    /// The byte offset in the buffer of the upright picture's pixel
    /// `(x, y)`, the picture being `size`.
    fn offset(&self, (x, y): (u32, u32), size: (u32, u32)) -> i64 {
        let (bx, by) = self.transform.buffer_position((x, y), size);
        let by = if self.y_inverted {
            self.height - 1 - by
        } else {
            by
        };
        i64::from(by) * i64::from(self.stride) + i64::from(bx) * 4
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

    /// Every transform, rows from the top or the bottom, a stride wider
    /// than a row: each pixel of the image is the one `buffer_position`
    /// names, read a pixel at a time, as the conversion was defined before
    /// it read whole rows.
    #[test]
    fn each_pixel_is_the_one_its_transform_names() {
        let (width, height, stride) = (5_u32, 3_u32, 24_u32);
        let bytes: Vec<u8> = (0..stride * height)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        for transform in Transform::ALL {
            for y_inverted in [false, true] {
                let buffer = Buffer {
                    bytes: &bytes,
                    width,
                    height,
                    stride,
                    layout: Layout::Rgbx,
                    y_inverted,
                    transform,
                };
                let image = buffer.to_image().unwrap();
                let size = (image.width(), image.height());
                for y in 0..size.1 {
                    for x in 0..size.0 {
                        let (bx, by) = transform.buffer_position((x, y), size);
                        let by = if y_inverted { height - 1 - by } else { by };
                        let at = (by * stride + bx * 4) as usize;
                        let wanted = Rgba::opaque(bytes[at], bytes[at + 1], bytes[at + 2]);
                        assert_eq!(
                            image.pixel(x, y),
                            Some(wanted),
                            "{transform:?}, inverted {y_inverted}, at ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}
