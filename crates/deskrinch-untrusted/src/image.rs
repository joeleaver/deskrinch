//! Decoded pixels and themed icon names, as an untrusted peer's input becomes
//! them once validated.

use crate::text;

/// Decoded pixels: `width` × `height`, tightly packed, 8-bit RGBA, not
/// premultiplied, rows top to bottom.
///
/// Every `Image` was validated on its way in: each wire format's decoder
/// (the notification service's `(iiibiiay)`, the status notifier's ARGB32)
/// checks the peer's sizes and data against its bounds before building one
/// with [`Image::new`].
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// An image of `rgba` pixels, `width` × `height`; `None` unless both are
    /// at least 1 and `rgba` holds exactly `width * height * 4` bytes.
    #[must_use]
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(usize::try_from(height).ok()?))
            .and_then(|n| n.checked_mul(4))?;
        (width >= 1 && height >= 1 && rgba.len() == expected).then_some(Self {
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
