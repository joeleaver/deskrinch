//! PNG through the `png` crate directly: its header read gives the size,
//! layout and profile before any pixel buffer exists, and its own limit
//! bounds what it allocates for itself (rows, the profile, chunks), which
//! `image` would not pass on to it (its `set_limits` checks dimensions
//! only).
//!
//! Any colour type and depth becomes 8-bit RGBA: the decoder expands
//! palettes, low depths and transparency, and narrows 16-bit samples to
//! their high byte as it writes each row (`Transformations::EXPAND |
//! STRIP_16`), so what it writes is never wider than the result. An APNG
//! gives its default image, the first one it shows. Text chunks are
//! skipped unread; `cICP`, `gAMA` and `cHRM` are not applied.

use std::io::Cursor;

use ::png::{BitDepth, ColorType, Decoder, DecodingError, Limits as PngLimits, Transformations};
use image::metadata::Orientation;

use crate::colour;
use crate::decode::{Embedded, Raw};
use crate::error::DecodeError;
use crate::limits::{Format, Limits};
use crate::rgba::{self, Channels};

/// What the decoder may allocate for itself besides its rows and the
/// profile: its inflate state and scratch.
const FIXED_BYTES: usize = 256 * 1024;

/// Rows of the widest layout (16-bit RGBA, 8 bytes a pixel) the decoder may
/// hold at once: the row being unfiltered, the one before it, and its
/// output line, with one to spare.
const ROWS: usize = 4;

/// Decodes the PNG `bytes` under `limits`.
pub(crate) fn decode(bytes: &[u8], limits: &Limits) -> Result<Raw, DecodeError> {
    let (width, height) =
        header_size(bytes).ok_or_else(|| DecodeError::malformed(Format::Png, "no PNG header"))?;
    let result_bytes = limits.result_bytes(width, height)?;
    let row = usize::try_from(width)
        .unwrap_or(usize::MAX)
        .saturating_mul(8);
    let own = FIXED_BYTES
        .saturating_add(row.saturating_mul(ROWS))
        .saturating_add(colour::MAX_PROFILE_BYTES);
    // The colour conversion's share is checked once the profile is known
    // (`decode::finish`); a PNG has no orientation.
    limits.working(result_bytes.checked_add(own))?;

    let mut decoder = Decoder::new_with_limits(Cursor::new(bytes), PngLimits { bytes: own });
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let fault = |error| fault(error, limits);
    let mut reader = decoder.read_info().map_err(fault)?;
    let info = reader.info();
    if (info.width, info.height) != (width, height) {
        return Err(DecodeError::malformed(Format::Png, "its header changed"));
    }
    let animated = info.animation_control.is_some();
    // The decoder drops a profile it cannot inflate within its limit, or
    // a corrupt one, without a word: whether there was one is read from
    // the chunks.
    let icc = match &info.icc_profile {
        Some(profile) => Embedded::Bytes(profile.to_vec()),
        None if has_profile_chunk(bytes) => Embedded::Unreadable(
            "the PNG decoder dropped it: corrupt, or larger than it may inflate",
        ),
        None => Embedded::None,
    };
    let native = reader
        .output_buffer_size()
        .ok_or_else(|| DecodeError::malformed(Format::Png, "no output size"))?;
    if native > result_bytes {
        return Err(DecodeError::Unsupported {
            format: Format::Png,
            what: "a layout wider than 8-bit RGBA after expansion",
        });
    }
    let mut rgba = vec![0; result_bytes];
    let frame = reader.next_frame(&mut rgba[..native]).map_err(fault)?;
    if !animated {
        reader.finish().map_err(fault)?;
    }
    if frame.bit_depth != BitDepth::Eight || (frame.width, frame.height) != (width, height) {
        return Err(DecodeError::malformed(Format::Png, "an unexpected frame"));
    }
    let channels = match frame.color_type {
        ColorType::Grayscale => Channels::Grey,
        ColorType::GrayscaleAlpha => Channels::GreyAlpha,
        ColorType::Rgb => Channels::Rgb,
        ColorType::Rgba => Channels::Rgba,
        ColorType::Indexed => {
            return Err(DecodeError::malformed(
                Format::Png,
                "a palette left unexpanded",
            ));
        }
    };
    let pixels = result_bytes / 4;
    if frame.line_size * usize::try_from(height).unwrap_or(usize::MAX) != pixels * channels.bytes()
    {
        return Err(DecodeError::malformed(Format::Png, "padded rows"));
    }
    rgba::widen(&mut rgba, pixels, channels);
    Ok(Raw {
        width,
        height,
        rgba,
        icc,
        orientation: Orientation::NoTransforms,
    })
}

/// The decoder's error as this crate's.
fn fault(error: DecodingError, limits: &Limits) -> DecodeError {
    match error {
        DecodingError::LimitsExceeded => DecodeError::WorkingTooLarge {
            needed: None,
            max: limits.max_working_bytes,
        },
        other => DecodeError::malformed(Format::Png, other),
    }
}

/// Whether the PNG `bytes` have an `iCCP` chunk before their image data:
/// the chunks walked by their lengths, nothing else read.
fn has_profile_chunk(bytes: &[u8]) -> bool {
    let mut at = 8;
    while let Some(header) = bytes.get(at..at + 8) {
        let (length, kind) = header.split_at(4);
        match kind {
            b"iCCP" => return true,
            b"IDAT" => return false,
            _ => {}
        }
        let length = u32::from_be_bytes([length[0], length[1], length[2], length[3]]);
        let Some(next) = usize::try_from(length)
            .ok()
            .and_then(|n| n.checked_add(12))
            .and_then(|n| n.checked_add(at))
        else {
            return false;
        };
        at = next;
    }
    false
}

/// The width and height the header of `bytes` gives: the first chunk must
/// be the 13-byte `IHDR`, whose first 8 bytes are they, big-endian.
fn header_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const HEADER: [u8; 8] = [0, 0, 0, 13, b'I', b'H', b'D', b'R'];
    let size = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    (bytes.get(8..16)? == HEADER).then_some(())?;
    Some((size(16)?, size(20)?))
}
