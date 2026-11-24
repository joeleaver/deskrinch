//! GIF through the `gif` crate directly: its first frame, drawn where it
//! sits on the logical screen (transparent elsewhere), with the decoder's
//! own per-frame memory limit set. An animation's later frames are never
//! decoded. A GIF's colour profile (an application extension few write) is
//! not read.
//!
//! The decoder collects the data of the application extensions before the
//! first frame (XMP and ICC today); the walk over the blocks
//! ([`walk::gif`]) says how much there is of every application extension,
//! and it is charged.
//!
//! The result is the logical screen's size; a screen of 0 × 0, which some
//! writers leave, takes the first frame's size. A frame reaching past the
//! screen is clipped to it.

use std::io::Cursor;
use std::num::NonZeroU64;
use std::sync::Arc;

use deskrinch_untrusted::{held, zeroed_pixels};

use ::gif::{ColorOutput, DecodeOptions, DecodingError, MemoryLimit};
use image::metadata::Orientation;

use crate::decode::{Embedded, Raw};
use crate::error::DecodeError;
use crate::limits::{Format, Limits};
use crate::walk;

/// What the decoder allocates besides the frame's buffers: its LZW tables,
/// a palette, the read buffer.
const FIXED_BYTES: usize = 128 * 1024;

/// What collecting application-extension data costs, as a multiple of the
/// data: a vector grown by doubling holds up to twice it, and the copy at
/// each growth holds the old beside the new.
const GROWTH: usize = 3;

/// Decodes the first frame of the GIF `bytes` under `limits`.
pub(crate) fn decode(bytes: &[u8], limits: &Limits) -> Result<Raw, DecodeError> {
    let malformed = |fault: DecodingError| DecodeError::malformed(Format::Gif, fault);
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    // The frame's own buffer is never larger than a whole result.
    let per_frame = u64::try_from(limits.max_decoded_bytes)
        .unwrap_or(u64::MAX)
        .max(1);
    options.set_memory_limit(MemoryLimit::Bytes(
        NonZeroU64::new(per_frame).unwrap_or(NonZeroU64::MIN),
    ));
    // The extensions before the first frame are read with its information,
    // and the decoder keeps what it collects of application extensions
    // (growing a vector: up to three times the data while it grows), so
    // that is charged before they are read, and held through the decode.
    let metadata = walk::gif(bytes).saturating_mul(GROWTH);
    limits.working(metadata.checked_add(FIXED_BYTES))?;
    let mut decoder = options.read_info(Cursor::new(bytes)).map_err(malformed)?;
    let screen = (u32::from(decoder.width()), u32::from(decoder.height()));
    let frame = decoder
        .next_frame_info()
        .map_err(malformed)?
        .ok_or_else(|| DecodeError::malformed(Format::Gif, "no frame"))?;
    let at = (u32::from(frame.left), u32::from(frame.top));
    let size = (u32::from(frame.width), u32::from(frame.height));
    let (width, height) = if screen.0 == 0 || screen.1 == 0 {
        size
    } else {
        screen
    };
    let result_bytes = limits.result_bytes(width, height)?;
    let fills = at == (0, 0) && size == (width, height);
    let frame_bytes = if fills {
        0
    } else {
        limits.result_bytes(size.0, size.1)?
    };
    // The decoder decodes a frame's palette indices, a byte a pixel, into
    // a buffer of its own before it expands them to RGBA.
    let indices = usize::try_from(size.0)
        .unwrap_or(usize::MAX)
        .saturating_mul(usize::try_from(size.1).unwrap_or(usize::MAX));
    limits.working(
        held(result_bytes)
            .checked_add(frame_bytes)
            .and_then(|n| n.checked_add(indices))
            .and_then(|n| n.checked_add(metadata))
            .and_then(|n| n.checked_add(FIXED_BYTES)),
    )?;

    // The result's own shared buffer, written in place: handed on without
    // a copy.
    let mut rgba = zeroed_pixels(result_bytes);
    let buffer = Arc::make_mut(&mut rgba);
    if fills {
        decoder.read_into_buffer(buffer).map_err(malformed)?;
    } else {
        let mut pixels = vec![0; frame_bytes];
        decoder.read_into_buffer(&mut pixels).map_err(malformed)?;
        draw(buffer, width, height, &pixels, at, size);
    }
    Ok(Raw {
        width,
        height,
        rgba,
        icc: Embedded::None,
        orientation: Orientation::NoTransforms,
    })
}

/// Draws the frame `pixels`, `size` at `at`, onto the transparent `canvas`,
/// `width` × `height`, clipped to it.
fn draw(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    pixels: &[u8],
    at: (u32, u32),
    size: (u32, u32),
) {
    let (x0, y0) = (at.0 as usize, at.1 as usize);
    let (fw, fh) = (size.0 as usize, size.1 as usize);
    let (w, h) = (width as usize, height as usize);
    if x0 >= w || y0 >= h {
        return;
    }
    let columns = fw.min(w - x0);
    for row in 0..fh.min(h - y0) {
        let from = row * fw * 4;
        let to = ((y0 + row) * w + x0) * 4;
        canvas[to..to + columns * 4].copy_from_slice(&pixels[from..from + columns * 4]);
    }
}
