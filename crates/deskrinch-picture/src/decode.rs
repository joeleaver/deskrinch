//! The decode: the bounds checked in order, the format's decoder, then the
//! colour profile and the orientation applied to the result.

use std::path::Path;

use deskrinch_untrusted::Image;
use deskrinch_untrusted::file::{FileError, read_bounded};
use image::metadata::Orientation;

use crate::colour::{self, Ignored, Profile};
use crate::error::DecodeError;
use crate::limits::{Format, Formats, Limits};
use crate::{gif, jpeg, orient, png, webp};

/// A decoded picture, and what was done with its colour profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The pixels: straight (not premultiplied) RGBA8, sRGB, upright.
    pub image: Image,
    /// The format it was in.
    pub format: Format,
    /// What happened to its colour profile.
    pub profile: Profile,
}

/// What a format's decoder hands on: the pixels as stored, and what is
/// still to be applied to them.
pub(crate) struct Raw {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `width * height * 4` bytes of RGBA8.
    pub(crate) rgba: Vec<u8>,
    /// The embedded ICC profile.
    pub(crate) icc: Embedded,
    /// How the stored pixels are to be shown.
    pub(crate) orientation: Orientation,
}

/// A picture's embedded ICC profile, as its decoder found it.
pub(crate) enum Embedded {
    /// It has none.
    None,
    /// The profile's bytes.
    Bytes(Vec<u8>),
    /// It has one the decoder could not read, for this reason.
    Unreadable(&'static str),
}

impl From<Option<Vec<u8>>> for Embedded {
    fn from(icc: Option<Vec<u8>>) -> Self {
        icc.map_or(Self::None, Self::Bytes)
    }
}

/// Decodes the picture `bytes` hold, if it is in one of `formats`, under
/// `limits`.
///
/// # Errors
///
/// [`DecodeError`]: a limit it is over (checked before the memory is
/// taken), a format not recognised or not accepted, or a damaged picture.
pub fn decode(bytes: &[u8], formats: Formats, limits: &Limits) -> Result<Decoded, DecodeError> {
    if bytes.len() > limits.max_encoded_bytes {
        return Err(DecodeError::EncodedTooLarge {
            size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            max: limits.max_encoded_bytes,
        });
    }
    let format = Format::sniff(bytes).ok_or(DecodeError::NotRecognised)?;
    if !formats.contains(format.flag()) {
        return Err(DecodeError::NotAccepted(format));
    }
    let raw = match format {
        Format::Png => png::decode(bytes, limits)?,
        Format::Jpeg => jpeg::decode(bytes, limits)?,
        Format::Gif => gif::decode(bytes, limits)?,
        Format::WebP => webp::decode(bytes, limits)?,
    };
    finish(raw, format, limits)
}

/// Decodes the picture in the file at `path`, a regular file of at most
/// `limits.max_encoded_bytes`, read without blocking
/// ([`read_bounded`]); otherwise as [`decode`].
///
/// # Errors
///
/// As [`decode`]; and [`DecodeError::File`] for a file that cannot be read
/// or is not a regular file, [`DecodeError::EncodedTooLarge`] for one over
/// the bound.
pub fn decode_file(path: &Path, formats: Formats, limits: &Limits) -> Result<Decoded, DecodeError> {
    let bytes = read_bounded(path, limits.max_encoded_bytes).map_err(|error| match error {
        FileError::TooLarge { size, max } => DecodeError::EncodedTooLarge { size, max },
        other => DecodeError::File(other),
    })?;
    decode(&bytes, formats, limits)
}

/// The profile converted (when what that costs fits the working bound;
/// otherwise it is ignored) and the orientation applied, which the
/// format's decoder already counted.
fn finish(raw: Raw, format: Format, limits: &Limits) -> Result<Decoded, DecodeError> {
    let Raw {
        width,
        height,
        mut rgba,
        icc,
        orientation,
    } = raw;
    let row = width as usize;
    let profile = match icc {
        Embedded::None => Profile::None,
        Embedded::Unreadable(why) => Profile::Ignored(Ignored::Malformed {
            fault: why.to_owned(),
        }),
        Embedded::Bytes(icc)
            if rgba.len().saturating_add(colour::extra(icc.len(), row))
                > limits.max_working_bytes =>
        {
            Profile::Ignored(Ignored::OverWorkingBound)
        }
        Embedded::Bytes(icc) => colour::convert(&mut rgba, row, &icc),
    };
    let (rgba, width, height) = orient::apply(rgba, width, height, orientation);
    // Every decoder hands on width × height × 4 bytes.
    let image = Image::new(width, height, rgba)
        .ok_or_else(|| DecodeError::malformed(format, "a result of the wrong size"))?;
    Ok(Decoded {
        image,
        format,
        profile,
    })
}
