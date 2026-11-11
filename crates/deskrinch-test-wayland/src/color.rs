//! A colour, as captures report pixels and test windows paint them.

use std::fmt;

/// One pixel: 8-bit sRGB channels with straight (not premultiplied) alpha.
///
/// What an output shows is opaque, so a captured pixel normally has alpha
/// 255; a compositor that leaves part of an output uncovered may report it
/// transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha: 0 is transparent, 255 opaque.
    pub a: u8,
}

impl Rgba {
    /// An opaque colour.
    #[must_use]
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// The colour from premultiplied channels, as Wayland buffers and
    /// KWin's premultiplied formats store them.
    #[must_use]
    pub fn from_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        let straight = |channel: u8| match a {
            0 => 0,
            255 => channel,
            _ => {
                let value = (u32::from(channel) * 255 + u32::from(a) / 2) / u32::from(a);
                u8::try_from(value.min(255)).unwrap_or(u8::MAX)
            }
        };
        Self {
            r: straight(r),
            g: straight(g),
            b: straight(b),
            a,
        }
    }

    /// The colour's premultiplied channels, red first.
    #[must_use]
    pub fn premultiplied(self) -> [u8; 4] {
        let times_alpha = |channel: u8| {
            let value = (u32::from(channel) * u32::from(self.a) + 127) / 255;
            u8::try_from(value).unwrap_or(u8::MAX)
        };
        [
            times_alpha(self.r),
            times_alpha(self.g),
            times_alpha(self.b),
            self.a,
        ]
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{:02x}{:02x}{:02x}{:02x}",
            self.r, self.g, self.b, self.a
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_round_trips_where_it_can() {
        let red = Rgba::opaque(200, 10, 0);
        assert_eq!(red.premultiplied(), [200, 10, 0, 255]);
        let [r, g, b, a] = red.premultiplied();
        assert_eq!(Rgba::from_premultiplied(r, g, b, a), red);

        let half = Rgba {
            r: 255,
            g: 0,
            b: 128,
            a: 128,
        };
        assert_eq!(half.premultiplied(), [128, 0, 64, 128]);
        assert_eq!(Rgba::from_premultiplied(128, 0, 64, 128), half);
        assert_eq!(Rgba::from_premultiplied(9, 9, 9, 0), Rgba::default());
    }

    #[test]
    fn colours_print_as_hex() {
        assert_eq!(Rgba::opaque(255, 0, 16).to_string(), "#ff0010ff");
    }
}
