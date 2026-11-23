//! The ordinary corpus's pictures, made by the build script
//! (`build.rs`), which includes this file; the library embeds what it made
//! (`hostile::ordinary`). Not a module of the library: making these takes
//! seconds in an unoptimised build (3.6 s idle, over 20 s at a load of 96),
//! and a test process that made them paid that in front of whatever it
//! timed.

use crate::case::{Case, case, noise};
use crate::png::{BitDepth, ColorType};
use crate::{fixtures, gif, icc, jpeg, png, webp};

/// Pictures of every format and layout the decoder takes, in the order
/// `hostile::ordinary` answers them.
pub(crate) fn make() -> Vec<Case> {
    let mut cases = small();
    cases.extend(large());
    cases
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
            "jpeg 1024 progressive 4:4:4",
            fixtures::GRADIENT_PROGRESSIVE_444_JPEG.to_vec(),
        ),
        case(
            "jpeg 1024 progressive 4:2:0",
            fixtures::GRADIENT_PROGRESSIVE_420_JPEG.to_vec(),
        ),
        case(
            "jpeg 1024 baseline",
            fixtures::GRADIENT_BASELINE_JPEG.to_vec(),
        ),
        case("webp 2048 lossy", fixtures::GRADIENT_LOSSY_WEBP.to_vec()),
        case(
            "webp 1024 lossy alpha",
            fixtures::GRADIENT_LOSSY_ALPHA_WEBP.to_vec(),
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
