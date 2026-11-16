//! The corpus the bounds are tested on: every kind of picture the decoder
//! takes, and what a stranger might send instead.

use std::sync::LazyLock;

use crate::png::{BitDepth, ColorType};
use crate::{fixtures, gif, icc, jpeg, png, webp};

/// A named picture of the corpus.
#[derive(Debug, Clone)]
pub struct Case {
    /// What it is.
    pub name: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

fn case(name: &str, bytes: Vec<u8>) -> Case {
    Case {
        name: name.to_owned(),
        bytes,
    }
}

/// Pixels that do not repeat, `n` bytes of them.
#[must_use]
pub fn noise(n: usize) -> Vec<u8> {
    let mut x: u32 = 0x9e37_79b9;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.to_le_bytes()[0]
        })
        .collect()
}

/// Pictures of every format and layout the decoder takes.
#[must_use]
pub fn ordinary() -> &'static [Case] {
    static ORDINARY: LazyLock<Vec<Case>> = LazyLock::new(|| {
        let mut cases = small();
        cases.extend(large());
        cases
    });
    &ORDINARY
}

/// Pictures large enough that what a decoder allocates per pixel, not its
/// fixed share, decides its peak.
fn large() -> Vec<Case> {
    let side = 512_u32;
    let pixels = (side * side) as usize;
    let rgba = noise(pixels * 4);
    let rgb = noise(pixels * 3);
    vec![
        case(
            "large png rgb",
            png::encode(side, side, ColorType::Rgb, BitDepth::Eight, &rgb),
        ),
        case(
            "large png interlaced",
            png::interlaced(side, side, true, png::distinct),
        ),
        case(
            "large webp lossless rgba",
            webp::lossless(side, side, &rgba),
        ),
        case(
            "large webp lossless rgb",
            webp::lossless_rgb(side, side, &rgb),
        ),
        case(
            "large webp lossy alpha",
            fixtures::LOSSY_ALPHA_NOISE_WEBP.to_vec(),
        ),
        case(
            "large webp animated",
            webp::animated(side, side, &[(0, 0, side, side, rgba.clone())]),
        ),
        case(
            "large gif",
            gif::encode(
                512,
                512,
                &[gif::Placed {
                    left: 0,
                    top: 0,
                    width: 512,
                    height: 512,
                    rgba: noise(pixels * 4),
                }],
            ),
        ),
        case(
            "large gif, offset frame",
            gif::encode(
                512,
                512,
                &[gif::Placed {
                    left: 2,
                    top: 2,
                    width: 500,
                    height: 500,
                    rgba: noise(500 * 500 * 4),
                }],
            ),
        ),
    ]
}

/// Small pictures of every format and layout.
fn small() -> Vec<Case> {
    let mut cases = small_jpeg_png();
    cases.extend(small_gif_webp());
    cases
}

/// Small JPEGs and PNGs of every layout.
fn small_jpeg_png() -> Vec<Case> {
    let side = 96_u32;
    let pixels = (side * side) as usize;
    let rgba = noise(pixels * 4);
    let rgb = noise(pixels * 3);
    let p3 = icc::display_p3();
    let big_rgb_profile = icc::lut(icc::Space::Rgb, 33);
    vec![
        case("jpeg baseline", fixtures::BASELINE_JPEG.to_vec()),
        case("jpeg progressive", fixtures::PROGRESSIVE_JPEG.to_vec()),
        case("jpeg 4:2:0", fixtures::SUBSAMPLED_JPEG.to_vec()),
        case("jpeg grey", fixtures::GREY_JPEG.to_vec()),
        case("jpeg cmyk", fixtures::CMYK_JPEG.to_vec()),
        case(
            "jpeg progressive noise",
            fixtures::PROGRESSIVE_NOISE_JPEG.to_vec(),
        ),
        case(
            "jpeg turned a quarter",
            jpeg::with_orientation(fixtures::PROGRESSIVE_NOISE_JPEG, 6),
        ),
        case(
            "jpeg display p3",
            jpeg::with_profile(fixtures::BASELINE_JPEG, &p3),
        ),
        case(
            "jpeg with a 33-point rgb table",
            jpeg::with_profile(fixtures::PROGRESSIVE_NOISE_JPEG, &big_rgb_profile),
        ),
        case(
            "png rgba",
            png::encode(side, side, ColorType::Rgba, BitDepth::Eight, &rgba),
        ),
        case(
            "png rgb",
            png::encode(side, side, ColorType::Rgb, BitDepth::Eight, &rgb),
        ),
        case(
            "png grey",
            png::encode(
                side,
                side,
                ColorType::Grayscale,
                BitDepth::Eight,
                &rgb[..pixels],
            ),
        ),
        case(
            "png rgba 16-bit",
            png::encode(side / 2, side, ColorType::Rgba, BitDepth::Sixteen, &rgba),
        ),
        case(
            "png interlaced",
            png::interlaced(side, side, true, png::distinct),
        ),
        case(
            "png adobe rgb",
            png::with_profile(
                side,
                side,
                ColorType::Rgb,
                BitDepth::Eight,
                &rgb,
                &icc::adobe_rgb(),
            ),
        ),
        case(
            "png with a 33-point rgb table",
            png::with_profile(
                side,
                side,
                ColorType::Rgb,
                BitDepth::Eight,
                &rgb,
                &big_rgb_profile,
            ),
        ),
    ]
}

/// Small APNG, GIF and WebP pictures of every layout.
fn small_gif_webp() -> Vec<Case> {
    let side = 96_u32;
    let pixels = (side * side) as usize;
    let rgba = noise(pixels * 4);
    let rgb = noise(pixels * 3);
    let frame = |left, top, w: u16, h: u16| gif::Placed {
        left,
        top,
        width: w,
        height: h,
        rgba: noise(usize::from(w) * usize::from(h) * 4),
    };
    vec![
        case("apng", png::animated(side, side, &[&rgba, &rgba])),
        case("gif", gif::encode(64, 64, &[frame(0, 0, 64, 64)])),
        case(
            "gif animated, offset frame",
            gif::encode(96, 64, &[frame(10, 7, 40, 30), frame(0, 0, 96, 64)]),
        ),
        case("webp lossless rgba", webp::lossless(side, side, &rgba)),
        case("webp lossless rgb", webp::lossless_rgb(side, side, &rgb)),
        case("webp lossy", fixtures::LOSSY_WEBP.to_vec()),
        case("webp lossy alpha", fixtures::LOSSY_ALPHA_WEBP.to_vec()),
        case(
            "webp animated",
            webp::animated(
                96,
                64,
                &[
                    (10, 6, 40, 30, noise(40 * 30 * 4)),
                    (0, 0, 96, 64, noise(96 * 64 * 4)),
                ],
            ),
        ),
    ]
}

/// What a stranger might send instead of a picture.
#[must_use]
pub fn hostile() -> Vec<Case> {
    let zeros_side = 1024_u32;
    let zeros = vec![0; (zeros_side * zeros_side * 4) as usize];
    let small = png::encode(4, 4, ColorType::Rgba, BitDepth::Eight, &[9; 64]);
    let mut huge_png = small.clone();
    huge_png[16..24].copy_from_slice(&[0, 1, 0x86, 0xa0, 0, 1, 0x86, 0xa0]);
    let small_webp = webp::lossless(4, 4, &[9; 64]);
    vec![
        case("png claiming 100000 × 100000", huge_png),
        case(
            "png bomb, 1024 × 1024 of zeros",
            png::encode(
                zeros_side,
                zeros_side,
                ColorType::Rgba,
                BitDepth::Eight,
                &zeros,
            ),
        ),
        case(
            "jpeg claiming 65000 × 65000",
            jpeg::with_size(fixtures::BASELINE_JPEG, 65_000, 65_000),
        ),
        case(
            "jpeg with more scans than the cap",
            jpeg::with_repeated_scan(fixtures::PROGRESSIVE_NOISE_JPEG, 80),
        ),
        case(
            "jpeg with as many scans as the cap allows",
            jpeg::with_repeated_scan(fixtures::PROGRESSIVE_NOISE_JPEG, 50),
        ),
        case(
            "jpeg with a table over the caps",
            jpeg::with_profile(fixtures::BASELINE_JPEG, &icc::lut(icc::Space::Rgb, 34)),
        ),
        case(
            "gif claiming a 65535 × 65535 screen",
            gif::with_screen(
                &gif::encode(
                    4,
                    4,
                    &[gif::Placed {
                        left: 0,
                        top: 0,
                        width: 4,
                        height: 4,
                        rgba: vec![9; 64],
                    }],
                ),
                65_535,
                65_535,
            ),
        ),
        case(
            "webp claiming 16000 × 16000",
            webp::with_size(&small_webp, 16_000, 16_000),
        ),
        case(
            "svg",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        ),
    ]
}
