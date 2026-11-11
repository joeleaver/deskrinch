use ::png::{BitDepth, ColorType, Encoder};
use proptest::prelude::*;

use super::*;

/// `data`, `width` × `height` of `color` at `depth`, encoded; with `setup`
/// applied to the encoder first.
fn encode_with(
    width: u32,
    height: u32,
    color: ColorType,
    depth: BitDepth,
    data: &[u8],
    setup: impl FnOnce(&mut Encoder<'_, &mut Vec<u8>>),
) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        setup(&mut encoder);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
    }
    out
}

fn encode(width: u32, height: u32, color: ColorType, depth: BitDepth, data: &[u8]) -> Vec<u8> {
    encode_with(width, height, color, depth, data, |_| {})
}

fn decoded(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    Image::from_png(bytes, 64).map(|i| (i.width(), i.height(), i.rgba().to_vec()))
}

#[test]
fn every_colour_type_and_depth_becomes_8_bit_rgba() {
    let rgba = |pixels: &[[u8; 4]]| Some((2, 1, pixels.concat()));
    let eight = BitDepth::Eight;
    assert_eq!(
        decoded(&encode(2, 1, ColorType::Grayscale, eight, &[0, 200])),
        rgba(&[[0, 0, 0, 255], [200, 200, 200, 255]]),
    );
    assert_eq!(
        decoded(&encode(
            2,
            1,
            ColorType::GrayscaleAlpha,
            eight,
            &[9, 1, 8, 2]
        )),
        rgba(&[[9, 9, 9, 1], [8, 8, 8, 2]]),
    );
    assert_eq!(
        decoded(&encode(2, 1, ColorType::Rgb, eight, &[1, 2, 3, 4, 5, 6])),
        rgba(&[[1, 2, 3, 255], [4, 5, 6, 255]]),
    );
    assert_eq!(
        decoded(&encode(
            2,
            1,
            ColorType::Rgba,
            eight,
            &[1, 2, 3, 4, 5, 6, 7, 8]
        )),
        rgba(&[[1, 2, 3, 4], [5, 6, 7, 8]]),
    );
    // 16-bit samples keep their high byte.
    let sixteen = [
        0x12, 0x34, 0, 0, 0xff, 0xff, 0x80, 0, 0, 0, 0, 0, 0, 0, 0x7f, 0xff,
    ];
    assert_eq!(
        decoded(&encode(2, 1, ColorType::Rgba, BitDepth::Sixteen, &sixteen)),
        rgba(&[[0x12, 0, 0xff, 0x80], [0, 0, 0, 0x7f]]),
    );
    // 1-bit greys are expanded to bytes.
    assert_eq!(
        decoded(&encode(
            2,
            1,
            ColorType::Grayscale,
            BitDepth::One,
            &[0b1000_0000]
        )),
        rgba(&[[255, 255, 255, 255], [0, 0, 0, 255]]),
    );
}

#[test]
fn a_palette_and_its_transparency_are_expanded() {
    let bytes = encode_with(2, 1, ColorType::Indexed, BitDepth::Eight, &[1, 0], |e| {
        e.set_palette(vec![10, 20, 30, 40, 50, 60]);
        e.set_trns(vec![0]);
    });
    assert_eq!(
        decoded(&bytes),
        Some((2, 1, [[40, 50, 60, 255], [10, 20, 30, 0]].concat())),
    );
}

#[test]
fn the_size_is_bounded_before_decoding() {
    let row = vec![0; 64 * 4];
    let wide = encode(64, 1, ColorType::Rgba, BitDepth::Eight, &row);
    assert!(Image::from_png(&wide, 64).is_some());
    assert_eq!(Image::from_png(&wide, 63), None);
    let high = encode(1, 64, ColorType::Rgba, BitDepth::Eight, &row);
    assert_eq!(Image::from_png(&high, 63), None);
    // A header claiming a huge image is refused without reading further.
    let mut huge = wide;
    huge[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(Image::from_png(&huge, u32::MAX), None);
}

#[test]
fn an_animation_is_refused() {
    let frame = [0, 0, 0, 255];
    let mut out = Vec::new();
    {
        let mut encoder = Encoder::new(&mut out, 1, 1);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&frame).unwrap();
        writer.write_image_data(&frame).unwrap();
        writer.finish().unwrap();
    }
    assert_eq!(decoded(&out), None);
}

#[test]
fn text_chunks_are_skipped() {
    let bytes = encode_with(1, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4], |e| {
        e.add_text_chunk("Comment".into(), "x".repeat(100_000))
            .unwrap();
    });
    assert_eq!(decoded(&bytes), Some((1, 1, vec![1, 2, 3, 4])));
}

#[test]
fn anything_but_a_whole_png_is_refused() {
    let whole = encode(2, 2, ColorType::Rgb, BitDepth::Eight, &[7; 12]);
    assert!(decoded(&whole).is_some());
    for end in 0..whole.len() {
        assert_eq!(decoded(&whole[..end]), None, "truncated at {end}");
    }
    assert_eq!(decoded(b"GIF89a not a png at all"), None);
}

proptest! {
    #![proptest_config(crate::fuzz::config())]

    #[test]
    fn arbitrary_bytes_never_panic_or_pass_the_bound(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        if let Some(image) = Image::from_png(&bytes, 16) {
            prop_assert!(image.width() <= 16 && image.height() <= 16);
        }
    }

    #[test]
    fn a_corrupted_png_never_panics_or_passes_the_bound(
        pixels in prop::collection::vec(any::<u8>(), 4 * 9),
        at in any::<prop::sample::Index>(),
        value in any::<u8>(),
        end in any::<prop::sample::Index>(),
    ) {
        let mut bytes = encode(3, 3, ColorType::Rgba, BitDepth::Eight, &pixels);
        let at = at.index(bytes.len());
        bytes[at] = value;
        bytes.truncate(end.index(bytes.len()) + 1);
        if let Some(image) = Image::from_png(&bytes, 3) {
            prop_assert_eq!((image.width(), image.height()), (3, 3));
        }
    }

    #[test]
    fn any_rgba_image_comes_back_as_it_went_in(
        (width, height, pixels) in (1_u32..8, 1_u32..8).prop_flat_map(|(w, h)| {
            (Just(w), Just(h), prop::collection::vec(any::<u8>(), (w * h * 4) as usize))
        }),
    ) {
        let bytes = encode(width, height, ColorType::Rgba, BitDepth::Eight, &pixels);
        prop_assert_eq!(decoded(&bytes), Some((width, height, pixels)));
    }
}
