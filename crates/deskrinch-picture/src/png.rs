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
//!
//! The decoder keeps every `eXIf` chunk it reads, before the image data
//! or after (`Reader::finish` reads those after); the walk over the chunks
//! ([`walk::png`]) says how much, and it is charged and added to the
//! decoder's own limit. An `eXIf` before the image data gives the
//! orientation.

use std::io::Cursor;

use ::png::{BitDepth, ColorType, Decoder, DecodingError, Limits as PngLimits, Transformations};
use image::metadata::Orientation;

use crate::colour::{self, Ignored};
use crate::decode::{Embedded, Raw};
use crate::error::DecodeError;
use crate::limits::{Format, Limits};
use crate::rgba::{self, Channels};
use crate::walk;

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
    let metadata = walk::png(bytes);
    let row = usize::try_from(width)
        .unwrap_or(usize::MAX)
        .saturating_mul(8);
    // The decoder's own share: its rows and scratch, the profile up to the
    // cap (what it inflates of a larger one is dropped, and said to be),
    // and every eXIf chunk it reads, before the image data or after.
    let own = FIXED_BYTES
        .saturating_add(row.saturating_mul(ROWS))
        .saturating_add(colour::MAX_PROFILE_BYTES)
        .saturating_add(metadata.exif);
    let orientation_extra = if metadata.exif_first { result_bytes } else { 0 };
    // It reads a chunk into a buffer of its own and copies an eXIf out of
    // it: two copies at once, of which its limit counts one.
    let needed = result_bytes.checked_add(own).map(|peak| {
        peak.saturating_add(metadata.exif).max(
            result_bytes
                .saturating_add(metadata.exif)
                .saturating_add(orientation_extra),
        )
    });
    limits.working(needed)?;

    let mut decoder = Decoder::new_with_limits(Cursor::new(bytes), PngLimits { bytes: own });
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(fault)?;
    let info = reader.info();
    if (info.width, info.height) != (width, height) {
        return Err(DecodeError::malformed(Format::Png, "its header changed"));
    }
    let animated = info.animation_control.is_some();
    // The decoder drops a profile it cannot inflate within its limit, or
    // a corrupt one, without a word: whether there was one is the walk's.
    let icc = match &info.icc_profile {
        Some(profile) => Embedded::Bytes(profile.to_vec()),
        None if metadata.profile => Embedded::Ignored(Ignored::Malformed {
            fault: "the PNG decoder dropped it: corrupt, or larger than it may inflate".to_owned(),
        }),
        None => Embedded::None,
    };
    // EXIF before the image data is what is shown; after it, not
    // (see the crate's documentation).
    let orientation = if metadata.exif_first {
        info.exif_metadata
            .as_deref()
            .map(strip_exif_prefix)
            .and_then(Orientation::from_exif_chunk)
            .unwrap_or(Orientation::NoTransforms)
    } else {
        Orientation::NoTransforms
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
        orientation,
    })
}

/// The decoder's error as this crate's. Over its own limit is not the
/// caller's bound (what the decoder keeps is charged to that before it
/// starts): it wanted more than this crate's estimate of a picture this
/// size allows.
fn fault(error: DecodingError) -> DecodeError {
    match error {
        DecodingError::LimitsExceeded => DecodeError::Unsupported {
            format: Format::Png,
            what: "it needs more of the decoder's memory than a picture of its size is allowed",
        },
        other => DecodeError::malformed(Format::Png, other),
    }
}

/// EXIF data without the `Exif\0\0` some writers put before it.
pub(crate) fn strip_exif_prefix(exif: &[u8]) -> &[u8] {
    exif.strip_prefix(b"Exif\0\0").unwrap_or(exif)
}

/// The width and height the header of `bytes` gives: the first chunk must
/// be the 13-byte `IHDR`, whose first 8 bytes are they, big-endian.
fn header_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const HEADER: [u8; 8] = [0, 0, 0, 13, b'I', b'H', b'D', b'R'];
    let size = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    (bytes.get(8..16)? == HEADER).then_some(())?;
    Some((size(16)?, size(20)?))
}
