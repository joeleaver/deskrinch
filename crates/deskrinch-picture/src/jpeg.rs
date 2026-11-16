//! JPEG through zune-jpeg directly: its header read says, before any pixel
//! buffer exists, the size, the number of components, whether it is
//! progressive, the EXIF orientation and the ICC profile; and it takes a
//! cap on the scans of a progressive file, which `image` does not pass on.
//!
//! What zune allocates for itself is bounded from those facts
//! ([`working`]): a baseline file needs rows of blocks, a progressive one
//! keeps every coefficient of the picture until its last scan. zune does
//! not say its components' sampling factors, so the bound takes the most
//! any component can be padded to, 32 pixels (four 8-pixel blocks) each
//! way. The working-bound test holds real and hostile files to it, so a
//! zune update that allocates more fails a test.
//!
//! The colour spaces zune decodes are all made RGBA: YCbCr and RGB as they
//! are, grey widened, CMYK and YCCK converted to RGB by zune (their print
//! profiles are not RGB and are ignored, [`crate::Ignored::NotRgb`]).

use image::metadata::Orientation;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::decode::{Embedded, Raw};
use crate::error::DecodeError;
use crate::limits::{Format, Limits};
use crate::orient;
use crate::rgba::{self, Channels};

/// The most scans a progressive JPEG may have: 64. Each scan walks the
/// coefficients of the whole picture again, so the decode's time grows
/// with the scans; the usual encoders write 10 (libjpeg's default script)
/// and mozjpeg's optimised scripts stay under 20. zune's own default is
/// 100; this crate sets its own.
pub const MAX_SCANS: usize = 64;

/// What zune allocates for itself however small the picture: its tables,
/// its markers' data, scratch.
const FIXED_BYTES: usize = 512 * 1024;

/// Decodes the JPEG `bytes` under `limits`.
pub(crate) fn decode(bytes: &[u8], limits: &Limits) -> Result<Raw, DecodeError> {
    let malformed = |fault| DecodeError::malformed(Format::Jpeg, fault);
    // The header read allocates nothing that grows with the size, so the
    // size is let through it and checked here, as a typed error; the
    // decode itself is held to the bound by zune too.
    let options = |max: usize| {
        DecoderOptions::default()
            .set_max_width(max)
            .set_max_height(max)
            .set_strict_mode(false)
            .jpeg_set_max_scans(MAX_SCANS)
    };
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options(usize::MAX));
    decoder.decode_headers().map_err(malformed)?;
    let info = decoder
        .info()
        .ok_or_else(|| malformed(zune_jpeg::errors::DecodeErrors::FormatStatic("no header")))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let result_bytes = limits.result_bytes(width, height)?;
    let input = decoder.input_colorspace().ok_or_else(|| {
        malformed(zune_jpeg::errors::DecodeErrors::FormatStatic(
            "no colour space",
        ))
    })?;
    let (out, channels) = match input {
        ColorSpace::Luma => (ColorSpace::Luma, Channels::Grey),
        ColorSpace::YCbCr | ColorSpace::RGB => (ColorSpace::RGBA, Channels::Rgba),
        // zune's CMYK conversion fills RGB; its RGBA leaves alpha at 0.
        ColorSpace::CMYK | ColorSpace::YCCK => (ColorSpace::RGB, Channels::Rgb),
        _ => {
            return Err(DecodeError::Unsupported {
                format: Format::Jpeg,
                what: "its colour space",
            });
        }
    };
    let orientation = info
        .exif_data
        .as_deref()
        .and_then(Orientation::from_exif_chunk)
        .unwrap_or(Orientation::NoTransforms);
    let icc = decoder.icc_profile();
    let icc_len = icc.as_ref().map_or(0, Vec::len);
    let own = working(
        width,
        height,
        usize::from(info.components),
        info.sof.is_progressive(),
    );
    let needed = own
        .and_then(|own| own.checked_add(result_bytes))
        .and_then(|peak| peak.checked_add(icc_len))
        .map(|peak| {
            let turned = result_bytes
                .saturating_add(icc_len)
                .saturating_add(orient::extra(orientation, result_bytes));
            peak.max(turned)
        });
    limits.working(needed)?;

    // zune chooses its colour conversion in its header read, so the decode
    // is a decoder of its own, made with the output colour space, reading
    // the headers again (they are small).
    let side = usize::try_from(limits.max_side).unwrap_or(usize::MAX);
    let mut decoder = JpegDecoder::new_with_options(
        ZCursor::new(bytes),
        options(side).jpeg_set_out_colorspace(out),
    );
    decoder.decode_headers().map_err(malformed)?;
    let mut rgba = vec![0; result_bytes];
    let native = result_bytes / 4 * channels.bytes();
    match decoder.output_buffer_size() {
        Some(buffer) if buffer == native => {}
        _ => {
            return Err(malformed(zune_jpeg::errors::DecodeErrors::FormatStatic(
                "an unexpected output size",
            )));
        }
    }
    decoder
        .decode_into(&mut rgba[..native])
        .map_err(malformed)?;
    rgba::widen(&mut rgba, result_bytes / 4, channels);
    Ok(Raw {
        width,
        height,
        rgba,
        icc: Embedded::from(icc),
        orientation,
    })
}

/// What zune allocates for itself decoding a `width` × `height` picture
/// of `components` components, besides the result: its fixed share, rows
/// of blocks for every component (a row of 8×8 blocks per vertical sample,
/// up to four, as 16-bit coefficients, twice over for the output stage),
/// and for a progressive file the coefficients of the whole picture.
/// `None` when that overflows.
fn working(width: u32, height: u32, components: usize, progressive: bool) -> Option<usize> {
    let pad = |side: u32| usize::try_from(side).ok()?.checked_next_multiple_of(32);
    let (w, h) = (pad(width)?, pad(height)?);
    let rows = components.checked_mul(w)?.checked_mul(32 * 2 * 2)?;
    let coefficients = if progressive {
        components.checked_mul(w)?.checked_mul(h)?.checked_mul(2)?
    } else {
        0
    };
    FIXED_BYTES.checked_add(rows)?.checked_add(coefficients)
}
