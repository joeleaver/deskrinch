//! Rows a decoder wrote with fewer than four channels, widened to RGBA8 in
//! the buffer they were written into: the decoder writes into the first
//! part of a buffer the size of the result, and the pixels are spread out
//! from the last one back, so no second buffer is needed.

/// The channels a decoder wrote per pixel, all 8-bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channels {
    /// Grey.
    Grey,
    /// Grey and alpha.
    GreyAlpha,
    /// Red, green, blue.
    Rgb,
    /// Red, green, blue, alpha: nothing to widen.
    Rgba,
}

impl Channels {
    /// Bytes per pixel.
    pub(crate) fn bytes(self) -> usize {
        match self {
            Self::Grey => 1,
            Self::GreyAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }
}

/// Widens the `pixels` pixels of `channels` packed at the start of `buffer`
/// (rows tightly packed) into RGBA8 filling the first `pixels * 4` bytes.
/// `buffer` holds at least `pixels * 4` bytes.
pub(crate) fn widen(buffer: &mut [u8], pixels: usize, channels: Channels) {
    let from = channels.bytes();
    if from == 4 {
        return;
    }
    // Back to front: pixel `i` is written at `i * 4` and up, and every pixel
    // still to be read (those before it) lies below `i * from`, so none is
    // overwritten before it is read.
    for i in (0..pixels).rev() {
        let at = i * from;
        let pixel = match channels {
            Channels::Grey => [buffer[at], buffer[at], buffer[at], u8::MAX],
            Channels::GreyAlpha => [buffer[at], buffer[at], buffer[at], buffer[at + 1]],
            Channels::Rgb => [buffer[at], buffer[at + 1], buffer[at + 2], u8::MAX],
            Channels::Rgba => return,
        };
        buffer[i * 4..i * 4 + 4].copy_from_slice(&pixel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_layout_widens_in_place() {
        let mut grey = vec![0; 8];
        grey[..2].copy_from_slice(&[7, 200]);
        widen(&mut grey, 2, Channels::Grey);
        assert_eq!(grey, [7, 7, 7, 255, 200, 200, 200, 255]);

        let mut grey_alpha = vec![0; 8];
        grey_alpha[..4].copy_from_slice(&[7, 1, 200, 2]);
        widen(&mut grey_alpha, 2, Channels::GreyAlpha);
        assert_eq!(grey_alpha, [7, 7, 7, 1, 200, 200, 200, 2]);

        let mut rgb = vec![0; 8];
        rgb[..6].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        widen(&mut rgb, 2, Channels::Rgb);
        assert_eq!(rgb, [1, 2, 3, 255, 4, 5, 6, 255]);

        let mut rgba = vec![1, 2, 3, 4];
        widen(&mut rgba, 1, Channels::Rgba);
        assert_eq!(rgba, [1, 2, 3, 4]);
    }
}
