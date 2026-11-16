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
//! zune keeps the file's metadata from its header read (the EXIF, the XMP,
//! the profile's pieces, more), and does not say how much without keeping
//! it; the walk over the file's segments ([`walk::jpeg`]) says, and the
//! decode charges it: the decode's decoder keeps it, and this crate keeps
//! its own copy of a profile within the cap. A profile over the cap is
//! never put together.
//!
//! The colour spaces zune decodes are all made RGBA: YCbCr and RGB as they
//! are, grey widened, CMYK and YCCK converted to RGB by zune (their print
//! profiles are not RGB and are ignored, [`crate::Ignored::NotRgb`]).

use image::metadata::Orientation;
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::colour::{self, Ignored};
use crate::decode::{Embedded, Raw};
use crate::error::DecodeError;
use crate::limits::{Format, Limits};
use crate::orient;
use crate::rgba::{self, Channels};
use crate::walk;

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
    // What zune will keep of the file's metadata, which it does not say
    // without keeping it.
    let metadata = walk::jpeg(bytes);
    let header = read_header(bytes)?;
    let result_bytes = limits.result_bytes(header.width, header.height)?;
    let (out, channels) = output(header.input)?;
    // A profile over the cap is not taken: its size is the walk's.
    let profile = if metadata.profile == 0 {
        Profile::None
    } else if metadata.profile > colour::MAX_PROFILE_BYTES {
        Profile::TooLarge
    } else {
        Profile::AfterDecode
    };
    let ours = if profile == Profile::AfterDecode {
        metadata.profile
    } else {
        0
    };
    // The decode's decoder keeps the metadata again (and puts extended XMP
    // together once more); this crate keeps its own copy of the profile.
    let kept = metadata
        .application
        .checked_add(metadata.extended_xmp)
        .and_then(|n| n.checked_add(ours));
    let own = working(
        header.width,
        header.height,
        header.components,
        header.progressive,
    );
    let needed = own
        .and_then(|own| own.checked_add(result_bytes))
        .and_then(|peak| peak.checked_add(kept?))
        .map(|peak| {
            let turned = result_bytes
                .saturating_add(ours)
                .saturating_add(orient::extra(header.orientation, result_bytes));
            peak.max(turned)
        });
    limits.working(needed)?;

    // zune chooses its colour conversion in its header read, so the decode
    // is a decoder of its own, made with the output colour space.
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
    let icc = match profile {
        Profile::None => Embedded::None,
        Profile::TooLarge => Embedded::Ignored(Ignored::TooLarge {
            size: metadata.profile,
        }),
        // The walk found its pieces; zune gives nothing when they do not
        // make a whole (one missing, or misnumbered).
        Profile::AfterDecode => match decoder.icc_profile() {
            Some(icc) => Embedded::Bytes(icc),
            None => Embedded::Ignored(Ignored::Malformed {
                fault: "its pieces do not make a whole profile".to_owned(),
            }),
        },
    };
    Ok(Raw {
        width: header.width,
        height: header.height,
        rgba,
        icc,
        orientation: header.orientation,
    })
}

/// zune's options: the size let through to `max` a side, lenient, with
/// this crate's scan cap.
fn options(max: usize) -> DecoderOptions {
    DecoderOptions::default()
        .set_max_width(max)
        .set_max_height(max)
        .set_strict_mode(false)
        .jpeg_set_max_scans(MAX_SCANS)
}

/// The header read. It keeps the file's metadata (at most the encoded size)
/// and nothing that grows with the picture's size, so the size is let
/// through it and checked by the caller, as a typed error; its decoder,
/// and its copy of the metadata, end when it returns.
fn read_header(bytes: &[u8]) -> Result<Header, DecodeError> {
    let malformed = |fault| DecodeError::malformed(Format::Jpeg, fault);
    let missing = |what| malformed(zune_jpeg::errors::DecodeErrors::FormatStatic(what));
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options(usize::MAX));
    decoder.decode_headers().map_err(malformed)?;
    let input = decoder
        .input_colorspace()
        .ok_or_else(|| missing("no colour space"))?;
    let info = decoder.info().ok_or_else(|| missing("no header"))?;
    Ok(Header {
        width: u32::from(info.width),
        height: u32::from(info.height),
        components: usize::from(info.components),
        progressive: info.sof.is_progressive(),
        input,
        orientation: orient::from_exif(info.exif_data.as_deref()),
    })
}

/// The colour space zune is to write for a file in `input`, and the
/// channels that is.
fn output(input: ColorSpace) -> Result<(ColorSpace, Channels), DecodeError> {
    Ok(match input {
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
    })
}

/// What is done with a JPEG's colour profile, as the walk finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Profile {
    /// It has none.
    None,
    /// Its pieces are over the cap: not put together.
    TooLarge,
    /// Its pieces are within the cap: put together after the decode.
    AfterDecode,
}

/// What the header read says.
struct Header {
    width: u32,
    height: u32,
    components: usize,
    progressive: bool,
    input: ColorSpace,
    orientation: Orientation,
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
