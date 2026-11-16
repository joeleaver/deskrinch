//! Segments spliced into a JPEG: what cameras and editors write, put into
//! the committed fixtures.

/// `jpeg` with the segment `marker` (0xE0 to 0xEF for an application
/// segment) of `payload` put right after its start-of-image marker.
#[must_use]
pub fn with_segment(jpeg: &[u8], marker: u8, payload: &[u8]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], [0xff, 0xd8], "a JPEG");
    let mut out = jpeg[..2].to_vec();
    out.extend([0xff, marker]);
    out.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// `jpeg` with an EXIF segment whose only tag is the orientation `value`
/// (1 to 8, or anything else for an invalid one).
#[must_use]
pub fn with_orientation(jpeg: &[u8], value: u16) -> Vec<u8> {
    let exif = [b"Exif\0\0".as_slice(), &exif_orientation(value)].concat();
    with_segment(jpeg, 0xe1, &exif)
}

/// EXIF data (a TIFF structure, as PNG's `eXIf` and WebP's `EXIF` chunks
/// hold it) whose only tag is the orientation `value`.
#[must_use]
pub fn exif_orientation(value: u16) -> Vec<u8> {
    // A little-endian TIFF header, its first IFD at 8, one entry: tag
    // 0x0112 (Orientation), type 3 (SHORT), count 1, the value; no next IFD.
    let mut exif = b"II*\0".to_vec();
    exif.extend(8_u32.to_le_bytes());
    exif.extend(1_u16.to_le_bytes());
    exif.extend(0x0112_u16.to_le_bytes());
    exif.extend(3_u16.to_le_bytes());
    exif.extend(1_u32.to_le_bytes());
    exif.extend(value.to_le_bytes());
    exif.extend([0, 0]);
    exif.extend(0_u32.to_le_bytes());
    exif
}

/// `jpeg` with the ICC profile `icc`, in as many APP2 segments as it takes
/// (each at most 65 519 bytes of profile).
#[must_use]
pub fn with_profile(jpeg: &[u8], icc: &[u8]) -> Vec<u8> {
    const PIECE: usize = 65_519 - 14;
    let pieces: Vec<&[u8]> = icc.chunks(PIECE).collect();
    let count = u8::try_from(pieces.len()).unwrap();
    // Spliced in reverse, each right after the start, so they end up in
    // order.
    let mut out = jpeg.to_vec();
    for (i, piece) in pieces.iter().enumerate().rev() {
        let mut payload = b"ICC_PROFILE\0".to_vec();
        payload.push(u8::try_from(i + 1).unwrap());
        payload.push(count);
        payload.extend_from_slice(piece);
        out = with_segment(&out, 0xe2, &payload);
    }
    out
}

/// The progressive `jpeg` with its first scan repeated `times` more times,
/// so it has that many scans more.
#[must_use]
pub fn with_repeated_scan(jpeg: &[u8], times: usize) -> Vec<u8> {
    let sos = find(jpeg, 0, &[0xff, 0xda]).unwrap();
    let next = find(jpeg, sos + 2, &[0xff, 0xda]).unwrap();
    let mut out = jpeg[..next].to_vec();
    for _ in 0..times {
        out.extend_from_slice(&jpeg[sos..next]);
    }
    out.extend_from_slice(&jpeg[next..]);
    out
}

/// `jpeg` with its frame header (SOF0 or SOF2) claiming `width` × `height`.
#[must_use]
pub fn with_size(jpeg: &[u8], width: u16, height: u16) -> Vec<u8> {
    let sof = find(jpeg, 0, &[0xff, 0xc0])
        .or_else(|| find(jpeg, 0, &[0xff, 0xc2]))
        .unwrap();
    let mut out = jpeg.to_vec();
    out[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
    out[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
    out
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|at| at + from)
}

/// `jpeg` with `count` segments `marker` of `payload` each, right after its
/// start-of-image marker.
#[must_use]
pub fn with_segments(jpeg: &[u8], marker: u8, payload: &[u8], count: usize) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    for _ in 0..count {
        out.extend([0xff, marker]);
        out.extend(u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(payload);
    }
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// `jpeg` (one with an Adobe APP14 segment, as the CMYK fixture has) with
/// its colour transform byte set to `transform`: 0 for CMYK or RGB, 1 for
/// YCbCr, 2 for YCCK.
#[must_use]
pub fn with_adobe_transform(jpeg: &[u8], transform: u8) -> Vec<u8> {
    // "Adobe", then a version (2 bytes), two flag words (4), the transform.
    let at = find(jpeg, 0, b"Adobe").unwrap() + 11;
    let mut out = jpeg.to_vec();
    out[at] = transform;
    out
}
