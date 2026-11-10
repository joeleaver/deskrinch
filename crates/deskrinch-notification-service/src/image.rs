//! Images and icons: pixel data validated before it becomes an [`Image`], and
//! references to files and themed icons.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;
use std::sync::Arc;

use crate::text;

/// Decoded pixels: `width` × `height`, tightly packed, 8-bit RGBA, not
/// premultiplied, rows top to bottom.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// Width in pixels; at least 1.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels; at least 1.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, `width * height * 4` bytes.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Validates a sender's `(iiibiiay)` image and decodes it, refusing one
    /// wider or taller than `max_side` pixels or with more than `max_bytes`
    /// of decoded RGBA. The size is checked before anything is allocated.
    pub(crate) fn decode(
        raw: &RawImage<'_>,
        max_side: u32,
        max_bytes: usize,
    ) -> Result<Self, ImageError> {
        let side = |v: i32| u32::try_from(v).ok().filter(|v| (1..=max_side).contains(v));
        let (Some(width), Some(height)) = (side(raw.width), side(raw.height)) else {
            return Err(ImageError::Dimensions {
                width: raw.width,
                height: raw.height,
            });
        };
        if raw.bits_per_sample != 8 {
            return Err(ImageError::BitsPerSample(raw.bits_per_sample));
        }
        let channels: usize = if raw.has_alpha { 4 } else { 3 };
        if usize::try_from(raw.channels).ok() != Some(channels) {
            return Err(ImageError::Channels {
                channels: raw.channels,
                has_alpha: raw.has_alpha,
            });
        }
        let (width_px, height_px) = (width as usize, height as usize);
        let decoded = width_px
            .checked_mul(height_px)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= max_bytes)
            .ok_or(ImageError::TooLarge { width, height })?;
        let row_bytes = width_px * channels;
        let stride = usize::try_from(raw.rowstride)
            .ok()
            .filter(|s| *s >= row_bytes)
            .ok_or(ImageError::Rowstride(raw.rowstride))?;
        // The last row need not be padded to the stride (gdk-pixbuf's rule).
        let shortest = stride
            .checked_mul(height_px - 1)
            .and_then(|n| n.checked_add(row_bytes))
            .ok_or(ImageError::Rowstride(raw.rowstride))?;
        let longest = stride
            .checked_mul(height_px)
            .ok_or(ImageError::Rowstride(raw.rowstride))?;
        if !(shortest..=longest).contains(&raw.data.len()) {
            return Err(ImageError::Length {
                expected: shortest,
                actual: raw.data.len(),
            });
        }
        let mut rgba = Vec::with_capacity(decoded);
        for row in 0..height_px {
            let start = row * stride;
            let pixels = &raw.data[start..start + row_bytes];
            if raw.has_alpha {
                rgba.extend_from_slice(pixels);
            } else {
                for px in pixels.as_chunks::<3>().0 {
                    rgba.extend_from_slice(px);
                    rgba.push(u8::MAX);
                }
            }
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// An image as the sender sent it, in the `(iiibiiay)` layout
/// gdk-pixbuf uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawImage<'a> {
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) rowstride: i32,
    pub(crate) has_alpha: bool,
    pub(crate) bits_per_sample: i32,
    pub(crate) channels: i32,
    pub(crate) data: &'a [u8],
}

/// Why a sender's image was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ImageError {
    #[error("{width}×{height} is not a supported size")]
    Dimensions { width: i32, height: i32 },
    #[error("{width}×{height} decodes to more bytes than one image may hold")]
    TooLarge { width: u32, height: u32 },
    #[error("{0} bits per sample, not 8")]
    BitsPerSample(i32),
    #[error("{channels} channels do not match has_alpha = {has_alpha}")]
    Channels { channels: i32, has_alpha: bool },
    #[error("row stride {0} is shorter than a row or too large")]
    Rowstride(i32),
    #[error("{actual} bytes of data where at least {expected} are needed")]
    Length { expected: usize, actual: usize },
}

/// Where an image or icon comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSource {
    /// Pixels the sender sent, validated and decoded.
    Pixels(Arc<Image>),
    /// A local file, given as an absolute path or a `file://` URI. The service
    /// does not read it; the UI decides whether to.
    File(PathBuf),
    /// A name in the freedesktop.org icon theme.
    Icon(IconName),
}

impl ImageSource {
    /// Parses an `app_icon` or `image-path` value: a `file://` URI, an
    /// absolute path, or an icon name. `None` for anything else, an empty
    /// string included.
    pub(crate) fn from_reference(reference: &str, max_bytes: usize) -> Option<Self> {
        if reference.len() > max_bytes {
            return None;
        }
        if reference.starts_with("file://") || reference.starts_with('/') {
            return file_reference(reference).map(Self::File);
        }
        IconName::parse(reference).map(Self::Icon)
    }
}

/// The local file a `file://` URI or an absolute path names; `None` for
/// anything else.
pub(crate) fn file_reference(reference: &str) -> Option<PathBuf> {
    if let Some(rest) = reference.strip_prefix("file://") {
        let path = rest.strip_prefix("localhost").unwrap_or(rest);
        return file_uri_path(path);
    }
    local_path(reference.as_bytes().to_vec())
}

/// A freedesktop.org icon theme name: non-empty, with no `/` or control
/// characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IconName(String);

impl IconName {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let valid = !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains('/')
            && text::clean(name, name.len(), text::Lines::Join) == name
            && !name.chars().any(char::is_whitespace);
        valid.then(|| Self(name.to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The path of a `file://` URI after its authority: absolute, with
/// percent-escapes decoded. Queries and fragments are not part of a file
/// reference, so a URI with either is refused.
fn file_uri_path(path: &str) -> Option<PathBuf> {
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return None;
    }
    // A `%` not followed by two hex digits stays literal, as RFC 3986
    // decoders treat it.
    local_path(percent_encoding::percent_decode_str(path).collect())
}

/// An absolute local path from raw bytes, refusing NUL.
fn local_path(bytes: Vec<u8>) -> Option<PathBuf> {
    if bytes.first() != Some(&b'/') || bytes.contains(&0) {
        return None;
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(test)]
mod tests;
