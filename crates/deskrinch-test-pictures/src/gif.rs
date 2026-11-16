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
