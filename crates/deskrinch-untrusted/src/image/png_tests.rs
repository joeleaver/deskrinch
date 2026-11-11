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

/// The Adam7 passes: first column and row, and the steps between them.
const PASSES: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// An Adam7-interlaced RGBA PNG, `width` × `height`, 16 bits a channel or
/// 8 (the high byte of each of `pixel`'s values). The `png` crate writes no
/// interlaced file, so it is put together here: rows unfiltered, the data
/// in stored deflate blocks, so no compressor is needed and the decoder,
/// the crate's own, checks it independently.
fn interlaced(
    width: u32,
    height: u32,
    sixteen: bool,
    pixel: impl Fn(u32, u32) -> [u16; 4],
) -> Vec<u8> {
    let steps = |size: u32, first: u32, step: u32| {
        if size > first {
            (size - first).div_ceil(step)
        } else {
            0
        }
    };
    let mut raw = Vec::new();
    for (x0, y0, dx, dy) in PASSES {
        let (columns, rows) = (steps(width, x0, dx), steps(height, y0, dy));
        if columns == 0 {
            continue;
        }
        for row in 0..rows {
            raw.push(0);
            for column in 0..columns {
                for channel in pixel(x0 + column * dx, y0 + row * dy) {
                    if sixteen {
                        raw.extend(channel.to_be_bytes());
                    } else {
                        raw.push(channel.to_be_bytes()[0]);
                    }
                }
            }
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(usize::from(u16::MAX)).collect();
    for (i, block) in blocks.iter().enumerate() {
        let len = u16::try_from(block.len()).unwrap();
        zlib.push(u8::from(i + 1 == blocks.len()));
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend(adler32(&raw).to_be_bytes());
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    // Depth, RGBA, deflate, adaptive filtering, Adam7.
    header.extend([if sixteen { 16 } else { 8 }, 6, 0, 0, 1]);
    let mut png = SIGNATURE.to_vec();
    chunk(&mut png, *b"IHDR", &header);
    chunk(&mut png, *b"IDAT", &zlib);
    chunk(&mut png, *b"IEND", &[]);
    png
}

fn chunk(png: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    png.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
    let start = png.len();
    png.extend_from_slice(&kind);
    png.extend_from_slice(data);
    let crc = crc32(&png[start..]);
    png.extend(crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// A colour for each place, different at each of a small image's pixels.
fn distinct(x: u32, y: u32) -> [u16; 4] {
    let byte = |v: u32| u16::try_from(v % 256).unwrap() << 8 | 0x5a;
    [
        byte(x * 50 + 7),
        byte(y * 60 + 3),
        byte(x * 31 + y * 17),
        byte(255 - x - y),
    ]
}

/// The high bytes of `pixel` over `width` × `height`, row by row: what the
/// decoder should give.
fn expected(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u16; 4]) -> Vec<u8> {
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(x, y).map(|c| c.to_be_bytes()[0]))
        .collect()
}

#[test]
fn an_interlaced_image_decodes_to_its_pixels() {
    // Smaller than one 8 × 8 block, so some passes are empty; and larger.
    for (width, height) in [(1, 1), (3, 3), (5, 2), (9, 11)] {
        for sixteen in [false, true] {
            let bytes = interlaced(width, height, sixteen, distinct);
            assert_eq!(
                decoded(&bytes),
                Some((width, height, expected(width, height, distinct))),
                "{width} × {height}, 16 bits: {sixteen}"
            );
        }
    }
}

#[test]
fn an_image_at_the_bound_fits_the_decoder_s_budget() {
    // The status notifier's default bound, at 16 bits a channel: the most
    // a decoder of an image this size is held to.
    const SIDE: u32 = 512;
    let pattern = |x: u32, y: u32| {
        let v = |n: u32| u16::try_from(n % 65_536).unwrap();
        [v(x * 129), v(y * 257), v(x * y), v(x + y * 3)]
    };
    let rows: Vec<u8> = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .flat_map(|(x, y)| pattern(x, y).into_iter().flat_map(u16::to_be_bytes))
        .collect();
    let plain = encode(SIDE, SIDE, ColorType::Rgba, BitDepth::Sixteen, &rows);
    let want = expected(SIDE, SIDE, pattern);
    let image = Image::from_png(&plain, SIDE).unwrap();
    assert_eq!(image.rgba(), want.as_slice());
    let interlaced = interlaced(SIDE, SIDE, true, pattern);
    let image = Image::from_png(&interlaced, SIDE).unwrap();
    assert_eq!(image.rgba(), want.as_slice());
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
