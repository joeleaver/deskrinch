//! Walks over a container's length-prefixed blocks, for what a decoder will
//! keep of a file's metadata where its own header read does not say: each
//! reads only lengths and short signatures, allocates nothing, and stops
//! at the end of the bytes or at a length that runs past them.

/// What a JPEG's segments hold that zune-jpeg 0.5.15 keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct JpegMetadata {
    /// Bytes of every application segment's payload: zune keeps the EXIF,
    /// the XMP, the ICC profile's pieces and more from them.
    pub(crate) application: usize,
    /// Bytes of extended XMP, which zune also puts together into one more
    /// copy.
    pub(crate) extended_xmp: usize,
    /// Bytes of the ICC profile's pieces (APP2 `ICC_PROFILE`), as the
    /// profile put together.
    pub(crate) profile: usize,
}

/// The JPEG `bytes`' metadata. Segments are found at the start and after
/// each other by their lengths; a scan's entropy-coded data, which has no
/// length, is passed over byte by byte, where a marker byte is always
/// stuffed or a restart marker, so the next segment is found where it
/// begins.
pub(crate) fn jpeg(bytes: &[u8]) -> JpegMetadata {
    const ICC: &[u8] = b"ICC_PROFILE\0";
    const EXTENDED_XMP: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
    let mut metadata = JpegMetadata::default();
    let mut at = 2;
    while at + 1 < bytes.len() {
        if bytes[at] != 0xff {
            at += 1;
            continue;
        }
        let marker = bytes[at + 1];
        // Fill bytes, stuffing, restarts, start and end of image, TEM:
        // markers without a length.
        if matches!(marker, 0x00 | 0x01 | 0xd0..=0xd9 | 0xff) {
            at += if marker == 0xff { 1 } else { 2 };
            continue;
        }
        let Some(length) = bytes
            .get(at + 2..at + 4)
            .map(|l| usize::from(u16::from_be_bytes([l[0], l[1]])))
        else {
            break;
        };
        let payload = bytes
            .get(at + 4..(at + 2 + length).min(bytes.len()))
            .unwrap_or(&[]);
        if (0xe0..=0xef).contains(&marker) {
            metadata.application += payload.len();
            if marker == 0xe2 && payload.starts_with(ICC) {
                metadata.profile += payload.len().saturating_sub(ICC.len() + 2);
            }
            if marker == 0xe1 && payload.starts_with(EXTENDED_XMP) {
                metadata.extended_xmp += payload.len();
            }
        }
        at += 2 + length.max(2);
    }
    metadata
}

/// What a PNG's chunks hold that the `png` decoder keeps or this crate uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PngMetadata {
    /// It has an `iCCP` chunk.
    pub(crate) profile: bool,
    /// Bytes of every `eXIf` chunk, before the image data or after it: the
    /// decoder reads (and keeps) each it reaches, and `Reader::finish`
    /// reaches those after the image data.
    pub(crate) exif: usize,
    /// An `eXIf` chunk comes before the first `IDAT`, so its orientation
    /// is applied.
    pub(crate) exif_first: bool,
}

/// The PNG `bytes`' metadata, from the chunks walked by their lengths.
pub(crate) fn png(bytes: &[u8]) -> PngMetadata {
    let mut metadata = PngMetadata::default();
    let mut seen_data = false;
    let mut at = 8;
    while let Some(header) = bytes.get(at..at + 8) {
        let length = usize::try_from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]))
        .unwrap_or(usize::MAX);
        match &header[4..8] {
            b"iCCP" => metadata.profile = true,
            b"eXIf" => {
                metadata.exif = metadata.exif.saturating_add(length);
                metadata.exif_first |= !seen_data;
            }
            b"IDAT" => seen_data = true,
            b"IEND" => break,
            _ => {}
        }
        let Some(next) = length.checked_add(12).and_then(|n| n.checked_add(at)) else {
            break;
        };
        at = next;
    }
    metadata
}

/// Bytes of application-extension data a GIF has before its first image:
/// what the `gif` decoder may collect (it keeps XMP and ICC data today;
/// every application extension is counted, so that one it starts to keep
/// is counted too).
pub(crate) fn gif(bytes: &[u8]) -> usize {
    let Some(&flags) = bytes.get(10) else {
        return 0;
    };
    let table = if flags & 0x80 == 0 {
        0
    } else {
        3 << ((flags & 7) + 1)
    };
    let mut at = 13 + table;
    let mut application = 0_usize;
    while let Some(&introducer) = bytes.get(at) {
        // Anything but an extension (the image descriptor, the trailer, or
        // damage) ends the walk.
        if introducer != 0x21 {
            break;
        }
        let Some(&label) = bytes.get(at + 1) else {
            break;
        };
        at += 2;
        // Sub-blocks: a length byte, that many bytes, until a length of 0.
        while let Some(&size) = bytes.get(at) {
            at += 1 + usize::from(size);
            if size == 0 {
                break;
            }
            if label == 0xff {
                application += usize::from(size);
            }
        }
    }
    application
}

/// What a WebP's chunks hold that this crate reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WebpMetadata {
    /// Bytes of its `ICCP` chunk.
    pub(crate) profile: usize,
    /// Bytes of its `EXIF` chunk, if it comes before the image data (an
    /// `ALPH`, `VP8 `, `VP8L` or `ANMF` chunk); one after it is not read.
    /// Of several, the last is image-webp's.
    pub(crate) exif_first: Option<usize>,
}

/// The WebP `bytes`' metadata, from the RIFF chunks walked by their sizes.
pub(crate) fn webp(bytes: &[u8]) -> WebpMetadata {
    let mut metadata = WebpMetadata::default();
    let mut seen_image = false;
    let mut at = 12;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = usize::try_from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]))
        .unwrap_or(usize::MAX);
        match &header[..4] {
            b"ICCP" => metadata.profile = metadata.profile.max(size),
            b"EXIF" => metadata.exif_first = (!seen_image).then_some(size),
            b"ALPH" | b"VP8 " | b"VP8L" | b"ANMF" => seen_image = true,
            _ => {}
        }
        // A chunk is padded to an even size.
        let Some(next) = size
            .checked_add(size % 2)
            .and_then(|n| n.checked_add(8))
            .and_then(|n| n.checked_add(at))
        else {
            break;
        };
        at = next;
    }
    metadata
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
