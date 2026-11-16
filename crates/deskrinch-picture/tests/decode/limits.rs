//! Each limit, and what it refuses, as a typed error.

use deskrinch_picture::{DecodeError, Format, Formats, Limits, decode};
use deskrinch_test_pictures::png::{BitDepth, ColorType, encode};
use deskrinch_test_pictures::{fixtures, gif, jpeg, webp};

fn limits() -> Limits {
    Limits {
        max_encoded_bytes: 1 << 20,
        max_side: 64,
        max_decoded_bytes: 64 * 64 * 4,
        max_working_bytes: 32 << 20,
    }
}

fn refused(bytes: &[u8], limits: &Limits) -> DecodeError {
    decode(bytes, Formats::all(), limits).unwrap_err()
}

#[test]
fn bytes_over_the_encoded_bound_are_refused_unread() {
    let tight = Limits {
        max_encoded_bytes: fixtures::BASELINE_JPEG.len() - 1,
        ..limits()
    };
    assert!(matches!(
        refused(fixtures::BASELINE_JPEG, &tight),
        DecodeError::EncodedTooLarge { max, .. } if max == tight.max_encoded_bytes
    ));
}

#[test]
fn a_header_over_the_side_is_refused_in_every_format() {
    let small_png = encode(4, 4, ColorType::Rgba, BitDepth::Eight, &[9; 64]);
    let mut huge_png = small_png.clone();
    huge_png[16..24].copy_from_slice(&[0, 1, 0x86, 0xa0, 0, 0, 0, 4]);
    let one = gif::Placed {
        left: 0,
        top: 0,
        width: 4,
        height: 4,
        rgba: vec![9; 64],
    };
    let cases: [(&str, Vec<u8>, (u32, u32)); 4] = [
        ("png", huge_png, (100_000, 4)),
        (
            "jpeg",
            jpeg::with_size(fixtures::BASELINE_JPEG, 65_000, 16),
            (65_000, 16),
        ),
        (
            "gif",
            gif::with_screen(&gif::encode(4, 4, &[one]), 65_535, 4),
            (65_535, 4),
        ),
        (
            "webp",
            webp::with_size(&webp::lossless(4, 4, &[9; 64]), 16_000, 4),
            (16_000, 4),
        ),
    ];
    for (name, bytes, size) in cases {
        match refused(&bytes, &limits()) {
            DecodeError::Dimensions {
                width,
                height,
                max_side,
            } => {
                assert_eq!((width, height), size, "{name}");
                assert_eq!(max_side, 64, "{name}");
            }
            other => panic!("{name}: {other}"),
        }
    }
}

#[test]
fn a_result_over_the_decoded_bound_is_refused() {
    let bytes = encode(64, 64, ColorType::Rgba, BitDepth::Eight, &[0; 64 * 64 * 4]);
    let tight = Limits {
        max_decoded_bytes: 64 * 64 * 4 - 1,
        ..limits()
    };
    assert!(matches!(
        refused(&bytes, &tight),
        DecodeError::DecodedTooLarge {
            width: 64,
            height: 64,
            ..
        }
    ));
}

#[test]
fn a_decode_over_the_working_bound_is_refused_before_it_starts() {
    let tight = Limits {
        max_working_bytes: 64 * 64 * 4,
        ..limits()
    };
    let bytes = encode(64, 64, ColorType::Rgba, BitDepth::Eight, &[0; 64 * 64 * 4]);
    assert!(matches!(
        refused(&bytes, &tight),
        DecodeError::WorkingTooLarge { max, .. } if max == tight.max_working_bytes
    ));
    assert!(matches!(
        refused(fixtures::PROGRESSIVE_JPEG, &tight),
        DecodeError::WorkingTooLarge { .. }
    ));
}

#[test]
fn what_is_not_one_of_the_four_is_not_recognised() {
    for bytes in [
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".as_slice(),
        b"",
        b"BM\0\0",
        b"RIFF\0\0\0\0WAVE",
    ] {
        assert!(matches!(
            refused(bytes, &limits()),
            DecodeError::NotRecognised
        ));
    }
}

#[test]
fn a_recognised_format_not_accepted_is_its_own_error() {
    for (bytes, format) in [
        (fixtures::BASELINE_JPEG.to_vec(), Format::Jpeg),
        (webp::lossless(1, 1, &[1, 2, 3, 4]), Format::WebP),
    ] {
        assert!(matches!(
            decode(&bytes, Formats::PNG | Formats::GIF, &limits()),
            Err(DecodeError::NotAccepted(f)) if f == format
        ));
    }
}
