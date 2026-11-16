//! Why a picture was refused.

use deskrinch_untrusted::file::FileError;

use crate::limits::Format;

/// Why a picture was not decoded: which limit it went over, or what was
/// wrong with it.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The encoded picture is larger than `max_encoded_bytes`.
    #[error("{size} bytes encoded, more than {max}")]
    EncodedTooLarge {
        /// Its size in bytes (for a file, at least the bound and one more).
        size: u64,
        /// The bound.
        max: usize,
    },
    /// The bytes are not one of the four formats: an SVG is "not
    /// recognised" too.
    #[error("not a format this decodes (PNG, JPEG, GIF, WebP)")]
    NotRecognised,
    /// A format this crate decodes, but not one the caller accepts.
    #[error("a {0:?} picture, which is not accepted here")]
    NotAccepted(Format),
    /// The header gives a width or height of 0 or over `max_side`.
    #[error("{width}×{height}: a side is 0 or over {max_side}")]
    Dimensions {
        /// The width the header gives.
        width: u32,
        /// The height the header gives.
        height: u32,
        /// The bound on each.
        max_side: u32,
    },
    /// The result, width × height × 4 bytes, would be over
    /// `max_decoded_bytes`.
    #[error("{width}×{height} decodes to more than {max} bytes")]
    DecodedTooLarge {
        /// The width.
        width: u32,
        /// The height.
        height: u32,
        /// The bound.
        max: usize,
    },
    /// The decode would allocate more than `max_working_bytes` at its peak.
    /// `needed` is `None` when that is not known: more than a `usize` can
    /// say, or a decoder found wanting more than was charged for what it
    /// keeps.
    #[error("decoding needs {needed:?} bytes at its peak, more than {max}")]
    WorkingTooLarge {
        /// What the header says the decode needs.
        needed: Option<usize>,
        /// The bound.
        max: usize,
    },
    /// A picture in a form this crate does not decode (a JPEG colour
    /// space, a PNG bit layout).
    #[error("a {format:?} picture this does not decode: {what}")]
    Unsupported {
        /// Its format.
        format: Format,
        /// What about it.
        what: &'static str,
    },
    /// The picture is damaged or not what its signature says: the
    /// decoder's own account.
    #[error("a damaged {format:?} picture: {fault}")]
    Malformed {
        /// Its format.
        format: Format,
        /// The decoder's account of the fault.
        fault: String,
    },
    /// Converting the picture's colour profile failed after part of the
    /// picture was converted, so its pixels are neither as decoded nor
    /// sRGB.
    #[error("its colour conversion failed part way: {fault}")]
    ColourConversion {
        /// moxcms's account.
        fault: String,
    },
    /// The named file was not read.
    #[error("the file: {0}")]
    File(FileError),
}

impl DecodeError {
    /// A [`DecodeError::Malformed`] of `format` with the decoder's `fault`.
    pub(crate) fn malformed(format: Format, fault: impl std::fmt::Display) -> Self {
        Self::Malformed {
            format,
            fault: fault.to_string(),
        }
    }
}
