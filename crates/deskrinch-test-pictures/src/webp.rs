//! WebP files: lossless ones through image-webp's encoder, and animations put
//! together here from them (the encoder writes none).

use image_webp::{ColorType, WebPEncoder};

/// `width` × `height` RGBA8 `rgba` as a lossless WebP.
#[must_use]
pub fn lossless(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    WebPEncoder::new(&mut out)
        .encode(rgba, width, height, ColorType::Rgba8)
        .unwrap();
    out
}

/// `width` × `height` RGB8 `rgb` as a lossless WebP without alpha.
#[must_use]
pub fn lossless_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    WebPEncoder::new(&mut out)
        .encode(rgb, width, height, ColorType::Rgb8)
        .unwrap();
    out
}

/// The `VP8L` chunk (header and payload) of a simple lossless WebP.
fn vp8l_chunk(webp: &[u8]) -> &[u8] {
    assert_eq!(&webp[12..16], b"VP8L", "a simple lossless WebP");
    &webp[12..]
}

/// An animated WebP on a `width` × `height` canvas of `frames`, each
/// `(left, top, frame width, frame height, rgba)` with an even `left` and
/// `top`, coded losslessly.
#[must_use]
pub fn animated(width: u32, height: u32, frames: &[(u32, u32, u32, u32, Vec<u8>)]) -> Vec<u8> {
    let three = |v: u32| v.to_le_bytes()[..3].to_vec();
    let mut body = b"WEBP".to_vec();
    // VP8X: animation and alpha, then the canvas size less one.
    let mut vp8x = vec![0b0001_0010, 0, 0, 0];
    vp8x.extend(three(width - 1));
    vp8x.extend(three(height - 1));
    chunk(&mut body, *b"VP8X", &vp8x);
    // ANIM: a transparent background, looping for ever.
    chunk(&mut body, *b"ANIM", &[0, 0, 0, 0, 0, 0]);
    for (left, top, w, h, rgba) in frames {
        let mut anmf = three(left / 2);
        anmf.extend(three(top / 2));
        anmf.extend(three(w - 1));
        anmf.extend(three(h - 1));
        anmf.extend(three(100));
        anmf.push(0);
        anmf.extend_from_slice(vp8l_chunk(&lossless(*w, *h, rgba)));
        chunk(&mut body, *b"ANMF", &anmf);
    }
    let mut out = b"RIFF".to_vec();
    out.extend(u32::try_from(body.len()).unwrap().to_le_bytes());
    out.extend(body);
    out
}

fn chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&kind);
    out.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// A simple lossless WebP of `width` × `height` with its header claiming
/// another size: the `VP8L` header's 14-bit width and height less one.
#[must_use]
pub fn with_size(webp: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = webp.to_vec();
    // RIFF header (12), chunk header (8), signature byte 0x2f, then the
    // 28 bits of width-1 and height-1, little-endian, and 4 more bits.
    let at = 21;
    let old = u32::from_le_bytes(out[at..at + 4].try_into().unwrap());
    let packed = (old & 0xf000_0000) | ((height - 1) << 14) | (width - 1);
    out[at..at + 4].copy_from_slice(&packed.to_le_bytes());
    out
}
