//! EXIF orientation applied to decoded pixels: the eight
//! orientations, the flips and the half turn in place, the quarter turns
//! (which swap width and height) through a second buffer the size of the
//! result, which the working bound counts ([`extra`]).

use image::metadata::Orientation;

/// The orientation EXIF data gives, as a decoder hands it over: a TIFF
/// structure, with or without the `Exif\0\0` some writers put before it.
/// No EXIF, or EXIF without a valid orientation, is shown as stored.
pub(crate) fn from_exif(exif: Option<&[u8]>) -> Orientation {
    exif.map(|exif| exif.strip_prefix(b"Exif\0\0").unwrap_or(exif))
        .and_then(Orientation::from_exif_chunk)
        .unwrap_or(Orientation::NoTransforms)
}

/// What applying `orientation` to a result of `decoded` bytes allocates.
pub(crate) fn extra(orientation: Orientation, decoded: usize) -> usize {
    if swaps_sides(orientation) { decoded } else { 0 }
}

/// Whether `orientation` turns the picture by a quarter, so its width and
/// height change places.
fn swaps_sides(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// `rgba`, `width` × `height`, shown as `orientation` says: the pixels,
/// and the width and height they now have.
pub(crate) fn apply(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    orientation: Orientation,
) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width as usize, height as usize);
    let mut rgba = rgba;
    match orientation {
        Orientation::NoTransforms => (rgba, width, height),
        Orientation::FlipHorizontal => {
            flip_rows(&mut rgba, w);
            (rgba, width, height)
        }
        Orientation::FlipVertical => {
            flip_columns(&mut rgba, w, h);
            (rgba, width, height)
        }
        Orientation::Rotate180 => {
            flip_rows(&mut rgba, w);
            flip_columns(&mut rgba, w, h);
            (rgba, width, height)
        }
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => {
            let mut out = vec![0; rgba.len()];
            for y in 0..h {
                for x in 0..w {
                    // Where (x, y) goes in the turned picture, `h` wide.
                    let (nx, ny) = match orientation {
                        Orientation::Rotate90 => (h - 1 - y, x),
                        Orientation::Rotate270 => (y, w - 1 - x),
                        Orientation::Rotate90FlipH => (y, x),
                        _ => (h - 1 - y, w - 1 - x),
                    };
                    let from = (y * w + x) * 4;
                    let to = (ny * h + nx) * 4;
                    out[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
                }
            }
            (out, height, width)
        }
    }
}

/// Mirrors each row left to right.
fn flip_rows(rgba: &mut [u8], width: usize) {
    for row in rgba.chunks_exact_mut(width * 4) {
        let (pixels, _) = row.as_chunks_mut::<4>();
        pixels.reverse();
    }
}

/// Mirrors the rows top to bottom.
fn flip_columns(rgba: &mut [u8], width: usize, height: usize) {
    let stride = width * 4;
    for y in 0..height / 2 {
        let (top, bottom) = rgba.split_at_mut((height - 1 - y) * stride);
        top[y * stride..(y + 1) * stride].swap_with_slice(&mut bottom[..stride]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3×2 picture whose pixels are numbered 1 to 6, row by row.
    fn numbered() -> Vec<u8> {
        (1..=6).flat_map(|n| [n, 0, 0, 255]).collect()
    }

    fn numbers(rgba: &[u8]) -> Vec<u8> {
        rgba.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
    }

    /// Each EXIF orientation is how the stored pixels must be turned to be
    /// shown upright (as the EXIF specification draws them).
    #[test]
    fn each_orientation_shows_the_picture_upright() {
        let cases: [(u8, (u32, u32), [u8; 6]); 8] = [
            (1, (3, 2), [1, 2, 3, 4, 5, 6]),
            (2, (3, 2), [3, 2, 1, 6, 5, 4]),
            (3, (3, 2), [6, 5, 4, 3, 2, 1]),
            (4, (3, 2), [4, 5, 6, 1, 2, 3]),
            (5, (2, 3), [1, 4, 2, 5, 3, 6]),
            (6, (2, 3), [4, 1, 5, 2, 6, 3]),
            (7, (2, 3), [6, 3, 5, 2, 4, 1]),
            (8, (2, 3), [3, 6, 2, 5, 1, 4]),
        ];
        for (exif, size, expected) in cases {
            let orientation = Orientation::from_exif(exif).unwrap();
            let (rgba, w, h) = apply(numbered(), 3, 2, orientation);
            assert_eq!((w, h), size, "orientation {exif}");
            assert_eq!(numbers(&rgba), expected, "orientation {exif}");
            let swaps = size != (3, 2);
            assert_eq!(extra(orientation, 24), if swaps { 24 } else { 0 });
        }
    }
}
