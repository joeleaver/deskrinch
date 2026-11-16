//! Decoded pixels and themed icon names, as an untrusted peer's input becomes
//! them once validated.

use std::sync::Arc;

use crate::text;

/// Decoded pixels: `width` × `height`, tightly packed, 8-bit RGBA, not
/// premultiplied, rows top to bottom.
///
/// Every `Image` was validated on its way in: each wire format's decoder
/// (the notification service's `(iiibiiay)`, the status notifier's ARGB32)
/// checks the peer's sizes and data against its bounds before building one
/// with [`Image::filled`], and encoded pictures are decoded under limits by
/// `deskrinch-picture`.
///
/// The pixels are one shared buffer: cloning an `Image` copies no pixel,
/// and [`Image::shared`] hands the buffer on (to rinch's `register_rgba`,
/// which keeps the same `Arc`) without a second copy.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

/// A buffer of `len` zero bytes, shared, made in one allocation (never a
/// `Vec` copied into an `Arc`): what [`Image::filled`] and a decoder that
/// writes into a buffer the size of its result start from. While it is not
/// yet shared, `Arc::make_mut` lends it out to be written in place.
#[must_use]
pub fn zeroed_pixels(len: usize) -> Arc<[u8]> {
    std::iter::repeat_n(0, len).collect()
}

/// What a shared buffer of pixels costs besides its bytes: the two
/// reference counts an `Arc` keeps in front of them, in the same
/// allocation. A bound on memory that holds such a buffer counts them
/// ([`held`]).
pub const SHARED_HEADER_BYTES: usize = 2 * size_of::<usize>();

/// The memory a shared buffer of `bytes` takes: the bytes and
/// [`SHARED_HEADER_BYTES`].
#[must_use]
pub fn held(bytes: usize) -> usize {
    bytes.saturating_add(SHARED_HEADER_BYTES)
}

/// `width * height * 4`, unless a side is 0 or it overflows.
fn bytes_for(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
        .and_then(|n| n.checked_mul(4))
}

impl Image {
    /// An image of `rgba` pixels, `width` × `height`; `None` unless both are
    /// at least 1 and `rgba` holds exactly `width * height * 4` bytes.
    ///
    /// An `Arc<[u8]>` is kept as it is. A `Vec<u8>` is copied into a shared
    /// buffer once, since an `Arc` cannot take over a `Vec`'s allocation:
    /// code that makes the pixels itself writes them in place instead
    /// ([`Image::filled`]).
    #[must_use]
    pub fn new(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Option<Self> {
        let expected = bytes_for(width, height)?;
        let rgba = rgba.into();
        (rgba.len() == expected).then_some(Self {
            width,
            height,
            rgba,
        })
    }

    /// An image `width` × `height` whose pixels `fill` writes, in place, into
    /// a zeroed buffer of exactly `width * height * 4` bytes; `None` for a
    /// side of 0 or a size that overflows (and `fill` is not called).
    #[must_use]
    pub fn filled(width: u32, height: u32, fill: impl FnOnce(&mut [u8])) -> Option<Self> {
        let mut rgba = zeroed_pixels(bytes_for(width, height)?);
        // Not shared yet: written in place, not copied.
        fill(Arc::make_mut(&mut rgba));
        Some(Self {
            width,
            height,
            rgba,
        })
    }

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

    /// The pixels' buffer itself, shared: another owner of the same bytes,
    /// not a copy.
    #[must_use]
    pub fn shared(&self) -> Arc<[u8]> {
        Arc::clone(&self.rgba)
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

/// A freedesktop.org icon theme name: non-empty, with no `/`, whitespace or
/// control characters, and neither `.` nor `..`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IconName(String);

impl IconName {
    /// `name` as an icon name, if it is one (see the type's documentation).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_holds_exactly_its_pixels() {
        let image = Image::new(2, 1, vec![0; 8]).unwrap();
        assert_eq!(
            (image.width(), image.height(), image.rgba().len()),
            (2, 1, 8)
        );
        assert_eq!(Image::new(2, 1, vec![0; 7]), None);
        assert_eq!(Image::new(0, 1, Vec::new()), None);
        assert_eq!(Image::new(u32::MAX, u32::MAX, Vec::new()), None);
    }

    #[test]
    fn a_filled_image_is_written_in_place_and_shared_not_copied() {
        let image = Image::filled(2, 1, |rgba| {
            assert_eq!(rgba.len(), 8);
            rgba[0] = 7;
        })
        .unwrap();
        assert_eq!(image.rgba(), &[7, 0, 0, 0, 0, 0, 0, 0]);
        let shared = image.shared();
        assert!(std::ptr::eq(shared.as_ptr(), image.rgba().as_ptr()));
        let copy = image.clone();
        assert!(std::ptr::eq(copy.rgba().as_ptr(), image.rgba().as_ptr()));
        assert_eq!(Image::filled(0, 1, |_| panic!("not called")), None);
    }

    #[test]
    fn a_shared_buffer_is_kept_as_it_is() {
        let pixels = zeroed_pixels(4);
        let image = Image::new(1, 1, Arc::clone(&pixels)).unwrap();
        assert!(Arc::ptr_eq(&image.shared(), &pixels));
    }

    #[test]
    fn icon_names_are_single_safe_words() {
        assert_eq!(
            IconName::parse("mail-client").unwrap().as_str(),
            "mail-client"
        );
        for bad in ["", ".", "..", "a/b", "a b", "a\u{7}"] {
            assert_eq!(IconName::parse(bad), None, "{bad:?}");
        }
    }
}
