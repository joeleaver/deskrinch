//! What a decode may cost, as the caller sets it, and the formats it accepts.

use crate::error::DecodeError;

/// What one decode may cost. Every value is the caller's: the notification
/// daemon and the wallpaper hold pictures to different bounds, and this
/// crate has no defaults.
///
/// The process's peak during a decode is the input (at most
/// `max_encoded_bytes`, held by the caller) plus at most `max_working_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of the encoded picture: the file, or the bytes handed over.
    pub max_encoded_bytes: usize,
    /// Pixels of the picture's width and of its height, as its header says.
    pub max_side: u32,
    /// Bytes of the result: width × height × 4.
    pub max_decoded_bytes: usize,
    /// Bytes the decode allocates at its peak, the result included and the
    /// input not: the decoder's buffers, a format's working set, the
    /// orientation's second buffer, the colour profile and its tables. A
    /// picture whose header says it would need more is refused before
    /// anything is allocated for it.
    pub max_working_bytes: usize,
}

impl Limits {
    /// The result's size, `width` × `height` × 4, if a picture that size
    /// is within the side and decoded-size bounds.
    pub(crate) fn result_bytes(&self, width: u32, height: u32) -> Result<usize, DecodeError> {
        if !(1..=self.max_side).contains(&width) || !(1..=self.max_side).contains(&height) {
            return Err(DecodeError::Dimensions {
                width,
                height,
                max_side: self.max_side,
            });
        }
        let bytes = usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
            .and_then(|n| n.checked_mul(4));
        match bytes {
            Some(bytes) if bytes <= self.max_decoded_bytes => Ok(bytes),
            _ => Err(DecodeError::DecodedTooLarge {
                width,
                height,
                max: self.max_decoded_bytes,
            }),
        }
    }

    /// `bytes` if a decode needing that much at its peak is within the
    /// working bound; `None` (an overflow) is over any bound.
    pub(crate) fn working(&self, bytes: Option<usize>) -> Result<usize, DecodeError> {
        match bytes {
            Some(bytes) if bytes <= self.max_working_bytes => Ok(bytes),
            _ => Err(DecodeError::WorkingTooLarge {
                needed: bytes,
                max: self.max_working_bytes,
            }),
        }
    }
}

bitflags::bitflags! {
    /// The formats a caller accepts. A picture in a format it recognises
    /// but the caller does not accept is refused as
    /// [`DecodeError::NotAccepted`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Formats: u8 {
        /// PNG, and APNG's first frame.
        const PNG = 1;
        /// JPEG (JFIF and EXIF), baseline and progressive.
        const JPEG = 1 << 1;
        /// GIF's first frame.
        const GIF = 1 << 2;
        /// WebP, lossy and lossless, and an animation's first frame.
        const WEBP = 1 << 3;
    }
}

/// A picture format this crate decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// PNG (Portable Network Graphics), APNG included.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF.
    Gif,
    /// WebP.
    WebP,
}

impl Format {
    /// The format `bytes` begin as, from their signature alone (never from
    /// a file name); `None` for anything else, an SVG included.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            Some(Self::WebP)
        } else {
            None
        }
    }

    /// The flag in [`Formats`] that accepts this format.
    #[must_use]
    pub fn flag(self) -> Formats {
        match self {
            Self::Png => Formats::PNG,
            Self::Jpeg => Formats::JPEG,
            Self::Gif => Formats::GIF,
            Self::WebP => Formats::WEBP,
        }
    }
}
