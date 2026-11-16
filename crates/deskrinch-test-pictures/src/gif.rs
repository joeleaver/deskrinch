//! GIFs through the `gif` crate's encoder.

use ::gif::{Encoder, Frame, Repeat};

/// A frame of a GIF: `width` × `height` RGBA8 `rgba` at (`left`, `top`).
/// Pixels with an alpha below 128 come out transparent.
#[derive(Debug, Clone)]
pub struct Placed {
    /// Where it sits on the screen.
    pub left: u16,
    /// Where it sits on the screen.
    pub top: u16,
    /// Its width.
    pub width: u16,
    /// Its height.
    pub height: u16,
    /// Its pixels.
    pub rgba: Vec<u8>,
}

/// A GIF with a logical screen of `width` × `height` and `frames`.
#[must_use]
pub fn encode(width: u16, height: u16, frames: &[Placed]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = Encoder::new(&mut out, width, height, &[]).unwrap();
        if frames.len() > 1 {
            encoder.set_repeat(Repeat::Infinite).unwrap();
        }
        for placed in frames {
            let mut rgba = placed.rgba.clone();
            let mut frame = Frame::from_rgba(placed.width, placed.height, &mut rgba);
            frame.left = placed.left;
            frame.top = placed.top;
            encoder.write_frame(&frame).unwrap();
        }
    }
    out
}

/// `gif` with its logical screen claiming `width` × `height`.
#[must_use]
pub fn with_screen(gif: &[u8], width: u16, height: u16) -> Vec<u8> {
    let mut out = gif.to_vec();
    out[6..8].copy_from_slice(&width.to_le_bytes());
    out[8..10].copy_from_slice(&height.to_le_bytes());
    out
}

/// `gif` with the extension `label` of `data` (in sub-blocks of at most
/// 255 bytes) put before its first frame; `prefix` is an application
/// extension's 11-byte identifier, written as its first sub-block.
#[must_use]
pub fn with_extension(gif: &[u8], label: u8, prefix: Option<&[u8; 11]>, data: &[u8]) -> Vec<u8> {
    // The header (6), the logical screen descriptor (7), and the global
    // colour table its flags announce.
    let flags = gif[10];
    let table = if flags & 0x80 == 0 {
        0
    } else {
        3 << ((flags & 7) + 1)
    };
    let after_screen = 13 + table;
    let mut out = gif[..after_screen].to_vec();
    out.extend([0x21, label]);
    if let Some(prefix) = prefix {
        out.push(11);
        out.extend_from_slice(prefix);
    }
    for block in data.chunks(255) {
        out.push(u8::try_from(block.len()).unwrap());
        out.extend_from_slice(block);
    }
    out.push(0);
    out.extend_from_slice(&gif[after_screen..]);
    out
}
