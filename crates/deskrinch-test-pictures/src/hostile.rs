//! The corpus the bounds are tested on: every kind of picture the decoder
//! takes, and what a stranger might send instead.

use std::sync::LazyLock;

use crate::case::case;
pub use crate::case::{Case, noise};
use crate::png::{BitDepth, ColorType};
use crate::{fixtures, gif, icc, jpeg, png, webp};

/// The ordinary corpus as the build script made it (`ordinary.rs`): each
/// picture's name and bytes, in order.
mod built {
    include!(concat!(env!("OUT_DIR"), "/ordinary.rs"));
}

/// Pictures of every format and layout the decoder takes: small ones, and
/// ones large enough that what a decoder allocates per pixel, not its fixed
/// share, decides its peak. They are made when this crate is built
/// (`ordinary.rs`), so a test pays only for a copy of their bytes, not for
/// encoding them.
#[must_use]
pub fn ordinary() -> &'static [Case] {
    static ORDINARY: LazyLock<Vec<Case>> = LazyLock::new(|| {
        built::ORDINARY
            .iter()
            .map(|&(name, bytes)| case(name, bytes.to_vec()))
            .collect()
    });
    &ORDINARY
}

/// Pictures that are mostly metadata: what each decoder keeps of it must be
/// counted, or refused, as the pixels are.
#[must_use]
pub fn metadata() -> &'static [Case] {
    static METADATA: LazyLock<Vec<Case>> = LazyLock::new(|| {
        const BIG: usize = 4 << 20;
        const SEGMENT: usize = 65_000;
        let junk = noise(BIG);
        let small = [9_u8; 16 * 16 * 4];
        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend_from_slice(&junk[..SEGMENT - xmp.len()]);
        let mut exif = b"Exif\0\0".to_vec();
        exif.extend_from_slice(&junk[..SEGMENT - exif.len()]);
        let png = png::encode(16, 16, ColorType::Rgba, BitDepth::Eight, &small);
        let mut splt = b"palette\0\x08".to_vec();
        splt.extend_from_slice(&junk[..BIG / 6 * 6]);
        let gif = gif::encode(
            16,
            16,
            &[gif::Placed {
                left: 0,
                top: 0,
                width: 16,
                height: 16,
                rgba: small.to_vec(),
            }],
        );
        vec![
            case(
                "jpeg with 4 MB of profile segments",
                jpeg::with_profile(fixtures::BASELINE_JPEG, &junk),
            ),
            case(
                "jpeg with 4 MB of APP1 and comment segments",
                jpeg::with_segments(
                    &jpeg::with_segments(fixtures::BASELINE_JPEG, 0xe1, &xmp, BIG / SEGMENT / 2),
                    0xfe,
                    &junk[..SEGMENT],
                    BIG / SEGMENT / 2,
                ),
            ),
            case(
                "jpeg with 4 MB of EXIF segments",
                jpeg::with_segments(fixtures::BASELINE_JPEG, 0xe1, &exif, BIG / SEGMENT),
            ),
            case(
                "webp with a 4 MB profile chunk",
                webp::extended(16, 16, &small, &junk, &[], &[]),
            ),
            case(
                "webp with 4 MB exif and xmp chunks",
                webp::extended(16, 16, &small, &[], &junk[..BIG / 2], &junk[BIG / 2..]),
            ),
            case(
                "gif with a 4 MB comment",
                gif::with_extension(&gif, 0xfe, None, &junk),
            ),
            case(
                "gif with a 4 MB application extension",
                gif::with_extension(&gif, 0xff, Some(b"XMP DataXMP"), &junk),
            ),
            case(
                "png with a 4 MB eXIf chunk",
                png::with_chunk(&png, *b"eXIf", &junk),
            ),
            case(
                "png with a 4 MB eXIf chunk after the image data",
                png::with_chunk_at_end(&png, *b"eXIf", &junk),
            ),
            case(
                "png with a 4 MB sPLT chunk",
                png::with_chunk(&png, *b"sPLT", &splt),
            ),
            case(
                "png with a 4 MB unknown chunk",
                png::with_chunk(&png, *b"drAg", &junk),
            ),
        ]
    });
    &METADATA
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
