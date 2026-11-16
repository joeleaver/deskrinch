//! PNGs: through the `png` crate's encoder, and Adam7-interlaced ones put
//! together here (the encoder writes none).

use std::borrow::Cow;

pub use ::png::{BitDepth, ColorType};
use ::png::{Encoder, Info};

/// `data`, `width` × `height` of `color` at `depth`, encoded, with `setup`
/// applied to the encoder first.
#[must_use]
pub fn encode_with(
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

/// `data`, `width` × `height` of `color` at `depth`, encoded.
#[must_use]
pub fn encode(width: u32, height: u32, color: ColorType, depth: BitDepth, data: &[u8]) -> Vec<u8> {
    encode_with(width, height, color, depth, data, |_| {})
}

/// `data` encoded with the ICC profile `icc` (an `iCCP` chunk).
#[must_use]
pub fn with_profile(
    width: u32,
    height: u32,
    color: ColorType,
    depth: BitDepth,
    data: &[u8],
    icc: &[u8],
) -> Vec<u8> {
    let mut info = Info::with_size(width, height);
    info.color_type = color;
    info.bit_depth = depth;
    info.icc_profile = Some(Cow::Owned(icc.to_vec()));
    let mut out = Vec::new();
    {
        let encoder = Encoder::with_info(&mut out, info).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
    }
    out
}

/// An APNG of `frames`, each `width` × `height` RGBA8.
#[must_use]
pub fn animated(width: u32, height: u32, frames: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = Encoder::new(&mut out, width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder
            .set_animated(u32::try_from(frames.len()).unwrap(), 0)
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        for frame in frames {
            writer.write_image_data(frame).unwrap();
        }
        writer.finish().unwrap();
    }
    out
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
/// 8 (the high byte of each of `pixel`'s values): rows unfiltered, the data
/// in stored deflate blocks, so no compressor is needed and the decoder
/// checks it independently.
#[must_use]
pub fn interlaced(
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
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    // Depth, RGBA, deflate, adaptive filtering, Adam7.
    header.extend([if sixteen { 16 } else { 8 }, 6, 0, 0, 1]);
    assemble(&header, &stored_zlib(&raw))
}

/// A PNG of the `IHDR` contents `header` and one `IDAT` of `data`.
#[must_use]
pub fn assemble(header: &[u8], data: &[u8]) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, *b"IHDR", header);
    chunk(&mut png, *b"IDAT", data);
    chunk(&mut png, *b"IEND", &[]);
    png
}

/// `raw` as a zlib stream of stored (uncompressed) blocks.
fn stored_zlib(raw: &[u8]) -> Vec<u8> {
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(usize::from(u16::MAX)).collect();
    for (i, block) in blocks.iter().enumerate() {
        let len = u16::try_from(block.len()).unwrap();
        zlib.push(u8::from(i + 1 == blocks.len()));
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend(adler32(raw).to_be_bytes());
    zlib
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
#[must_use]
pub fn distinct(x: u32, y: u32) -> [u16; 4] {
    let byte = |v: u32| u16::try_from(v % 256).unwrap() << 8 | 0x5a;
    [
        byte(x * 50 + 7),
        byte(y * 60 + 3),
        byte(x * 31 + y * 17),
        byte(255_u32.wrapping_sub(x + y)),
    ]
}

/// The high bytes of `pixel` over `width` × `height`, row by row.
#[must_use]
pub fn expected(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u16; 4]) -> Vec<u8> {
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(x, y).map(|c| c.to_be_bytes()[0]))
        .collect()
}

/// `png` with a chunk of `kind` holding `data` put right after its `IHDR`.
#[must_use]
pub fn with_chunk(png: &[u8], kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    // The signature (8) and the IHDR chunk (4 + 4 + 13 + 4).
    let after_header = 33;
    let mut out = png[..after_header].to_vec();
    chunk(&mut out, kind, data);
    out.extend_from_slice(&png[after_header..]);
    out
}

/// `png` with a chunk of `kind` holding `data` put right before its `IEND`,
/// after the image data.
#[must_use]
pub fn with_chunk_at_end(png: &[u8], kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    // The IEND chunk: length (4), type (4), CRC (4).
    let end = png.len() - 12;
    let mut out = png[..end].to_vec();
    chunk(&mut out, kind, data);
    out.extend_from_slice(&png[end..]);
    out
}
