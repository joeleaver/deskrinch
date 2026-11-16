//! PNG: every colour type and depth, palettes, interlacing, APNG, text.

use deskrinch_picture::{DecodeError, Format, Formats, Limits, Profile, decode};
use deskrinch_test_pictures::png::{
    BitDepth, ColorType, animated, distinct, encode, encode_with, expected, interlaced,
};

use crate::decoded;

fn rgba_of(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoded = decoded(bytes).unwrap();
    assert_eq!(decoded.format, Format::Png);
    assert_eq!(decoded.profile, Profile::None);
    (
        decoded.image.width(),
        decoded.image.height(),
        decoded.image.rgba().to_vec(),
    )
}

#[test]
fn every_colour_type_and_depth_becomes_8_bit_rgba() {
    let two = |pixels: [[u8; 4]; 2]| (2, 1, pixels.concat());
    let eight = BitDepth::Eight;
    assert_eq!(
        rgba_of(&encode(2, 1, ColorType::Grayscale, eight, &[0, 200])),
        two([[0, 0, 0, 255], [200, 200, 200, 255]]),
    );
    assert_eq!(
        rgba_of(&encode(
            2,
            1,
            ColorType::GrayscaleAlpha,
            eight,
            &[9, 1, 8, 2]
        )),
        two([[9, 9, 9, 1], [8, 8, 8, 2]]),
    );
    assert_eq!(
        rgba_of(&encode(2, 1, ColorType::Rgb, eight, &[1, 2, 3, 4, 5, 6])),
        two([[1, 2, 3, 255], [4, 5, 6, 255]]),
    );
    assert_eq!(
        rgba_of(&encode(
            2,
            1,
            ColorType::Rgba,
            eight,
            &[1, 2, 3, 4, 5, 6, 7, 8]
        )),
        two([[1, 2, 3, 4], [5, 6, 7, 8]]),
    );
    // 16-bit samples keep their high byte.
    let sixteen = [
        0x12, 0x34, 0, 0, 0xff, 0xff, 0x80, 0, 0, 0, 0, 0, 0, 0, 0x7f, 0xff,
    ];
    assert_eq!(
        rgba_of(&encode(2, 1, ColorType::Rgba, BitDepth::Sixteen, &sixteen)),
        two([[0x12, 0, 0xff, 0x80], [0, 0, 0, 0x7f]]),
    );
    // 1-bit greys are expanded to bytes.
    assert_eq!(
        rgba_of(&encode(
            2,
            1,
            ColorType::Grayscale,
            BitDepth::One,
            &[0b1000_0000]
        )),
        two([[255, 255, 255, 255], [0, 0, 0, 255]]),
    );
}

#[test]
fn a_palette_and_its_transparency_are_expanded() {
    let bytes = encode_with(2, 1, ColorType::Indexed, BitDepth::Eight, &[1, 0], |e| {
        e.set_palette(vec![10, 20, 30, 40, 50, 60]);
        e.set_trns(vec![0]);
    });
    assert_eq!(
        rgba_of(&bytes),
        (2, 1, [[40, 50, 60, 255], [10, 20, 30, 0]].concat()),
    );
}

#[test]
fn an_interlaced_image_decodes_to_its_pixels() {
    // Smaller than one 8 × 8 block, so some passes are empty; and larger.
    for (width, height) in [(1, 1), (3, 3), (5, 2), (9, 11), (64, 33)] {
        for sixteen in [false, true] {
            assert_eq!(
                rgba_of(&interlaced(width, height, sixteen, distinct)),
                (width, height, expected(width, height, distinct)),
                "{width} × {height}, 16 bits: {sixteen}"
            );
        }
    }
}

#[test]
fn an_animation_gives_its_first_frame() {
    let first = [1, 2, 3, 255];
    let second = [9, 9, 9, 9];
    let bytes = animated(1, 1, &[&first, &second]);
    assert_eq!(rgba_of(&bytes), (1, 1, first.to_vec()));
}

#[test]
fn text_chunks_are_skipped() {
    let bytes = encode_with(1, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4], |e| {
        e.add_text_chunk("Comment".into(), "x".repeat(100_000))
            .unwrap();
    });
    assert_eq!(rgba_of(&bytes), (1, 1, vec![1, 2, 3, 4]));
}

#[test]
fn anything_but_a_whole_png_is_refused() {
    let whole = encode(2, 2, ColorType::Rgb, BitDepth::Eight, &[7; 12]);
    assert!(decoded(&whole).is_ok());
    for end in 0..whole.len() {
        assert!(decoded(&whole[..end]).is_err(), "truncated at {end}");
    }
}

#[test]
fn a_png_is_refused_where_only_jpeg_is_accepted() {
    let bytes = encode(1, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4]);
    let limits = Limits {
        max_encoded_bytes: 1024,
        max_side: 8,
        max_decoded_bytes: 1024,
        max_working_bytes: 1 << 20,
    };
    assert!(matches!(
        decode(&bytes, Formats::JPEG, &limits),
        Err(DecodeError::NotAccepted(Format::Png))
    ));
    assert!(decode(&bytes, Formats::PNG, &limits).is_ok());
}
