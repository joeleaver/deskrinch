//! What the tests share: roomy limits, and pixels read back.

use deskrinch_picture::{DecodeError, Decoded, Formats, Limits, decode};

/// Limits every test picture is within.
pub(crate) fn roomy() -> Limits {
    Limits {
        max_encoded_bytes: 4 << 20,
        max_side: 1024,
        max_decoded_bytes: 4 << 20,
        max_working_bytes: 64 << 20,
    }
}

/// `bytes` decoded within [`roomy`] limits, any format.
pub(crate) fn decoded(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    decode(bytes, Formats::all(), &roomy())
}

/// The pixel at (`x`, `y`) of `decoded`.
pub(crate) fn pixel(decoded: &Decoded, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * decoded.image.width() + x) * 4) as usize;
    decoded.image.rgba()[at..at + 4].try_into().unwrap()
}

/// Whether each channel of `got` is within `tolerance` of `want`'s.
pub(crate) fn near(got: [u8; 4], want: [u8; 4], tolerance: u8) -> bool {
    got.iter()
        .zip(want)
        .all(|(g, w)| g.abs_diff(w) <= tolerance)
}

/// The colours at the centres of the four quadrants of a 32 × 16 fixture.
pub(crate) fn quadrants(decoded: &Decoded) -> [[u8; 4]; 4] {
    [
        pixel(decoded, 8, 4),
        pixel(decoded, 24, 4),
        pixel(decoded, 8, 12),
        pixel(decoded, 24, 12),
    ]
}
