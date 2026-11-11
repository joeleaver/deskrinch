//! PNG bytes from an untrusted peer, decoded into an [`Image`] only when
//! they are one plain, whole image within bounds.

use std::io::Cursor;

use ::png::{BitDepth, ColorType, Decoder, Limits, Transformations};

use super::Image;

/// The PNG signature, and where the header's size sits after it: the
/// first chunk must be the 13-byte `IHDR`, whose first 8 bytes are the
/// width and height, big-endian.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
const HEADER: [u8; 8] = [0, 0, 0, 13, b'I', b'H', b'D', b'R'];

/// What the decoder may allocate beyond the image's own rows (its
/// inflate state and scratch), however small the image.
const DECODER_OVERHEAD: usize = 256 * 1024;

impl Image {
    /// The PNG image `bytes` hold, if they hold exactly one, no wider or
    /// higher than `max_side`; any colour type and depth, made 8-bit RGBA.
    ///
    /// The size is read from the header and checked before anything is
    /// allocated, and the decoder is held to what an image of that size
    /// needs. Refused: anything that is not a PNG or is truncated, an
    /// animation (APNG), and any file that does not decode cleanly through
    /// its end. Text and colour-profile chunks are skipped unread.
    #[must_use]
    pub fn from_png(bytes: &[u8], max_side: u32) -> Option<Self> {
        let (width, height) = header_size(bytes)?;
        if !(1..=max_side).contains(&width) || !(1..=max_side).contains(&height) {
            return None;
        }
        // Rows of up to 16-bit RGBA, before the depth is stripped.
        let pixels = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?;
        let budget = pixels.checked_mul(8)?.checked_add(DECODER_OVERHEAD)?;
        let mut decoder = Decoder::new_with_limits(Cursor::new(bytes), Limits { bytes: budget });
        decoder.set_ignore_text_chunk(true);
        decoder.set_ignore_iccp_chunk(true);
        decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let info = reader.info();
        if info.animation_control.is_some() || (info.width, info.height) != (width, height) {
            return None;
        }
        let mut buffer = vec![0; reader.output_buffer_size()?];
        let frame = reader.next_frame(&mut buffer).ok()?;
        reader.finish().ok()?;
        if frame.bit_depth != BitDepth::Eight || (frame.width, frame.height) != (width, height) {
            return None;
        }
        let rgba = to_rgba(&buffer, frame.color_type, frame.line_size, width, height)?;
        Self::new(width, height, rgba)
    }
}

/// The width and height `bytes`' header gives, if they start as a PNG.
fn header_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let size = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    (bytes.get(..8)? == SIGNATURE && bytes.get(8..16)? == HEADER).then_some(())?;
    Some((size(16)?, size(20)?))
}

/// Expanded 8-bit rows of `color` (no palette is left after `EXPAND`),
/// `line` bytes apart, as tightly packed RGBA.
fn to_rgba(rows: &[u8], color: ColorType, line: usize, width: u32, height: u32) -> Option<Vec<u8>> {
    let channels = match color {
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        ColorType::Indexed => return None,
    };
    let width = usize::try_from(width).ok()?;
    let height = usize::try_from(height).ok()?;
    let mut rgba = Vec::with_capacity(width.checked_mul(height)?.checked_mul(4)?);
    for row in rows.chunks(line).take(height) {
        for pixel in row
            .get(..width.checked_mul(channels)?)?
            .chunks_exact(channels)
        {
            let [r, g, b, a] = match *pixel {
                [y] => [y, y, y, 255],
                [y, a] => [y, y, y, a],
                [r, g, b] => [r, g, b, 255],
                [r, g, b, a] => [r, g, b, a],
                _ => return None,
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    Some(rgba)
}

#[cfg(test)]
#[path = "png_tests.rs"]
mod tests;
