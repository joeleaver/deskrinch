//! WebP through image-webp directly. Its header read says the size, whether
//! there is alpha, whether it is lossy, lossless or animated, and gives the
//! ICC profile, before any pixel buffer exists; `image` passes it no limit
//! at all, and its own `set_memory_limit` bounds only metadata chunks.
//!
//! So what it allocates for itself rests on how image-webp 0.2.4 decodes,
//! not on a limit it enforces, and [`working`] says it per kind:
//!
//! - lossy: the Y, U and V planes, padded to 16-pixel macroblocks
//!   (1.5 bytes a pixel), and what it keeps per macroblock besides: 2 bytes
//!   a padded pixel together, as measured (1.86 on a 2048 × 2048 picture);
//!   with alpha, the alpha plane and the lossless stream it may be coded in
//!   (5 bytes a pixel);
//! - lossless: its transforms' sub-images and tables (at most 1 byte a
//!   pixel), and without alpha a whole RGBA copy it narrows to RGB from
//!   (4 bytes a pixel);
//! - animated (the first frame only): its canvas and the frame's own RGBA
//!   buffer (8 bytes a pixel), and the frame's decode as above.
//!
//! The working-bound test holds files of each kind to it, so an image-webp
//! update that allocates more fails a test.
//!
//! Metadata: the profile is read under image-webp's memory limit at the
//! profile cap (a larger one is ignored, its size the walk's,
//! [`walk::webp`]); an EXIF chunk is read only when it comes before the
//! image data, which in a WebP written to its specification it never does
//! (see the crate's documentation), and is charged; XMP is never read.

use std::io::Cursor;
use std::sync::Arc;

use deskrinch_untrusted::{held, zeroed_pixels};

use image::metadata::Orientation;
use image_webp::WebPDecoder;

use crate::colour::{self, Ignored};
use crate::decode::{Embedded, Raw};
use crate::error::DecodeError;
use crate::limits::{Format, Limits};
use crate::rgba::{self, Channels};
use crate::{orient, walk};

/// What the decoder allocates for itself however small the picture.
const FIXED_BYTES: usize = 512 * 1024;

/// Decodes the WebP `bytes` (an animation's first frame) under `limits`.
pub(crate) fn decode(bytes: &[u8], limits: &Limits) -> Result<Raw, DecodeError> {
    let malformed = |fault: image_webp::DecodingError| DecodeError::malformed(Format::WebP, fault);
    let metadata = walk::webp(bytes);
    let mut decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(malformed)?;
    let (width, height) = decoder.dimensions();
    let result_bytes = limits.result_bytes(width, height)?;
    let alpha = decoder.has_alpha();
    let animated = decoder.is_animated();
    let lossy = !animated && decoder.is_lossy();
    // The profile within the cap and an EXIF before the image data are
    // read, and kept through the decode.
    let profile = if metadata.profile <= colour::MAX_PROFILE_BYTES {
        metadata.profile
    } else {
        0
    };
    let exif = metadata.exif_first.unwrap_or(0);
    let turned = if metadata.exif_first.is_some() {
        held(result_bytes)
    } else {
        0
    };
    let kept = profile.saturating_add(exif);
    let needed = working(width, height, alpha, lossy, animated)
        .and_then(|own| own.checked_add(held(result_bytes)))
        .and_then(|peak| peak.checked_add(kept))
        .map(|peak| {
            peak.max(
                held(result_bytes)
                    .saturating_add(kept)
                    .saturating_add(turned),
            )
        });
    limits.working(needed)?;

    let icc = if metadata.profile > colour::MAX_PROFILE_BYTES {
        Embedded::Ignored(Ignored::TooLarge {
            size: metadata.profile,
        })
    } else {
        // Its read is held to the cap whatever the chunk index says.
        decoder.set_memory_limit(colour::MAX_PROFILE_BYTES);
        match decoder.icc_profile() {
            Ok(icc) => Embedded::from(icc),
            Err(fault) => Embedded::Ignored(Ignored::Malformed {
                fault: fault.to_string(),
            }),
        }
    };
    // An EXIF chunk after the image data is never read (see the crate's
    // documentation); one before it is read under what was charged for it.
    let orientation = if metadata.exif_first.is_some() {
        decoder.set_memory_limit(exif);
        let read = decoder.exif_metadata().map_err(|error| match error {
            image_webp::DecodingError::MemoryLimitExceeded => DecodeError::WorkingTooLarge {
                needed: None,
                max: limits.max_working_bytes,
            },
            other => DecodeError::malformed(Format::WebP, other),
        })?;
        orient::from_exif(read.as_deref())
    } else {
        Orientation::NoTransforms
    };

    let channels = if alpha { Channels::Rgba } else { Channels::Rgb };
    let native = decoder
        .output_buffer_size()
        .filter(|size| *size == result_bytes / 4 * channels.bytes())
        .ok_or_else(|| DecodeError::malformed(Format::WebP, "an unexpected output size"))?;
    // The result's own shared buffer, written in place: handed on without
    // a copy.
    let mut rgba = zeroed_pixels(result_bytes);
    let buffer = Arc::make_mut(&mut rgba);
    // An animation's first frame: `read_image` reads it and leaves the
    // decoder as it was.
    decoder
        .read_image(&mut buffer[..native])
        .map_err(malformed)?;
    rgba::widen(buffer, result_bytes / 4, channels);
    Ok(Raw {
        width,
        height,
        rgba,
        icc,
        orientation,
    })
}

/// What image-webp allocates for itself decoding a `width` × `height`
/// picture of this kind, besides the result (see the module's
/// documentation). `None` when that overflows.
fn working(width: u32, height: u32, alpha: bool, lossy: bool, animated: bool) -> Option<usize> {
    let pixels = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    let padded = |side: u32| usize::try_from(side).ok()?.checked_next_multiple_of(16);
    let planes = padded(width)?
        .checked_mul(padded(height)?)?
        .checked_mul(2)?;
    let lossy_bytes =
        |alpha: bool| planes.checked_add(if alpha { pixels.checked_mul(5)? } else { 0 });
    let lossless_bytes =
        |alpha: bool| pixels.checked_add(if alpha { 0 } else { pixels.checked_mul(4)? });
    let own = if animated {
        // The first frame may be of either kind, with or without alpha,
        // whatever the canvas says.
        let frame = lossy_bytes(true)?.max(lossless_bytes(false)?);
        pixels.checked_mul(8)?.checked_add(frame)?
    } else if lossy {
        lossy_bytes(alpha)?
    } else {
        lossless_bytes(alpha)?
    };
    own.checked_add(FIXED_BYTES)
}
