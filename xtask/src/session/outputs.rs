//! What a session's outputs are from the start: how many, how large, at
//! what scale, turned how.

use std::fmt;
use std::str::FromStr;

use super::{OutputTransform, Scale};

/// An output's size in device pixels: its mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputSize {
    /// Its width.
    pub(crate) width: u32,
    /// Its height.
    pub(crate) height: u32,
}

impl OutputSize {
    /// The size every output has unless one is asked for: the mode a
    /// compositor's headless output starts with here.
    pub(crate) const DEFAULT: Self = Self {
        width: 1280,
        height: 720,
    };
}

impl fmt::Display for OutputSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Why a size could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SizeError {
    /// Not `WIDTHxHEIGHT` in whole numbers.
    #[error("{0:?} is not a size: give it as WIDTHxHEIGHT, such as 1920x1080")]
    Malformed(String),
    /// A width or height of 0.
    #[error("{0} has no area: a width and a height of at least 1")]
    Zero(String),
}

impl FromStr for OutputSize {
    type Err = SizeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let malformed = || SizeError::Malformed(text.to_owned());
        let (width, height) = text.split_once('x').ok_or_else(malformed)?;
        let number = |part: &str| {
            part.chars()
                .all(|c| c.is_ascii_digit())
                .then(|| part.parse::<u32>().ok())
                .flatten()
                .ok_or_else(malformed)
        };
        let (width, height) = (number(width)?, number(height)?);
        if width == 0 || height == 0 {
            return Err(SizeError::Zero(text.to_owned()));
        }
        Ok(Self { width, height })
    }
}

/// The outputs' sizes asked for, on the command line: one per output, in
/// order (`--output-size`), for `session`, `measure` and
/// `compositor-tests` alike.
#[derive(Debug, Clone, Default, PartialEq, Eq, clap::Args)]
pub(crate) struct SizeArgs {
    /// An output's size in device pixels, its mode, as `WIDTHxHEIGHT`
    /// (repeatable: the first output's, then the second's, ...; an output
    /// given none is 1280x720). The scale divides it into the logical
    /// size. The outputs stay side by side. KWin's are set through
    /// `kscreen-doctor` once it runs (a custom mode), sway's in its
    /// configuration, and labwc's through wlr-output-management, all
    /// before anything else starts; the session fails (70) if an output
    /// then reports another.
    #[arg(long = "output-size", value_name = "WxH")]
    pub(crate) sizes: Vec<OutputSize>,
}

/// More sizes asked for than there are outputs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{sizes} output size(s) asked for {count} output(s): give one per output at most")]
pub(crate) struct TooManySizes {
    /// How many sizes.
    pub(crate) sizes: usize,
    /// How many outputs.
    pub(crate) count: u32,
}

/// The most outputs a session has.
pub(crate) const MAX_OUTPUTS: usize = 8;

/// A session's outputs, side by side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Outputs {
    /// How many.
    pub(crate) count: u32,
    /// Their scale.
    pub(crate) scale: Scale,
    /// Their transform.
    pub(crate) transform: OutputTransform,
    /// The sizes asked for, the first outputs' in order; an output given
    /// none is [`OutputSize::DEFAULT`]. None beyond `count`.
    sizes: [Option<OutputSize>; MAX_OUTPUTS],
}

impl Outputs {
    /// `count` outputs at `scale`, turned by `transform`, of the default
    /// size ([`sized`](Self::sized) gives them others).
    pub(crate) fn new(count: u32, scale: Scale, transform: OutputTransform) -> Self {
        Self {
            count,
            scale,
            transform,
            sizes: [None; MAX_OUTPUTS],
        }
    }

    /// `count` outputs at scale 1, untransformed, of the default size.
    pub(crate) fn plain(count: u32) -> Self {
        Self::new(count, Scale::ONE, OutputTransform::default())
    }

    /// These outputs, the first ones `sizes` large.
    pub(crate) fn sized(self, sizes: &[OutputSize]) -> Result<Self, TooManySizes> {
        let count = usize::try_from(self.count).unwrap_or(usize::MAX);
        if sizes.len() > count.min(MAX_OUTPUTS) {
            return Err(TooManySizes {
                sizes: sizes.len(),
                count: self.count,
            });
        }
        let mut outputs = self;
        for (slot, size) in outputs.sizes.iter_mut().zip(sizes) {
            *slot = Some(*size);
        }
        Ok(outputs)
    }

    /// Whether they are as every compositor starts them.
    pub(crate) fn is_plain(self) -> bool {
        self.scale.is_one() && self.transform.is_normal() && !self.has_sizes()
    }

    /// Whether any output has a size asked for.
    pub(crate) fn has_sizes(self) -> bool {
        self.sizes.iter().any(Option::is_some)
    }

    /// The size of the output at `index` (from 0).
    pub(crate) fn size(self, index: usize) -> OutputSize {
        self.sizes
            .get(index)
            .copied()
            .flatten()
            .unwrap_or(OutputSize::DEFAULT)
    }

    /// Each output's size and the position of its top-left corner in
    /// logical pixels, side by side from the left, in order: an output's
    /// logical width is its size turned with the transform and divided by
    /// the scale, rounded as compositors round it.
    pub(crate) fn layout(self) -> Vec<(OutputSize, (i32, i32))> {
        let mut x = 0_i32;
        (0..usize::try_from(self.count).unwrap_or(0))
            .map(|index| {
                let size = self.size(index);
                let across = if self.transform.get().swaps_axes() {
                    size.height
                } else {
                    size.width
                };
                let placed = (size, (x, 0));
                let logical = (f64::from(across) / self.scale.get()).round();
                // Bounded by the size, which is a `u32` of pixels.
                #[expect(clippy::cast_possible_truncation, reason = "a logical width")]
                {
                    x += logical as i32;
                }
                placed
            })
            .collect()
    }
}

impl Outputs {
    /// How many there are, and their sizes where any was asked for:
    /// `2 output(s)`, `2 output(s) of 3840x2160, 1280x720`.
    pub(crate) fn counted(self) -> String {
        if !self.has_sizes() {
            return format!("{} output(s)", self.count);
        }
        let sizes: Vec<String> = (0..usize::try_from(self.count).unwrap_or(0))
            .map(|index| self.size(index).to_string())
            .collect();
        format!("{} output(s) of {}", self.count, sizes.join(", "))
    }
}

impl fmt::Display for Outputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at scale {}", self.counted(), self.scale)?;
        if !self.transform.is_normal() {
            write!(f, ", transform {}", self.transform)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use deskrinch_test_wayland::Transform;

    use super::*;

    fn size(text: &str) -> OutputSize {
        text.parse().unwrap()
    }

    #[test]
    fn says_what_they_are() {
        assert_eq!(Outputs::plain(2).to_string(), "2 output(s) at scale 1");
        let turned = Outputs::new(1, Scale::ONE, Transform::Rotated90.into());
        assert_eq!(turned.to_string(), "1 output(s) at scale 1, transform 90");
        assert!(Outputs::plain(1).is_plain() && !turned.is_plain());
        let sized = Outputs::plain(2).sized(&[size("3840x2160")]).unwrap();
        assert_eq!(
            sized.to_string(),
            "2 output(s) of 3840x2160, 1280x720 at scale 1"
        );
        assert!(!sized.is_plain());
    }

    #[test]
    fn reads_a_size_and_refuses_what_is_not_one() {
        assert_eq!(
            size("1920x1080"),
            OutputSize {
                width: 1920,
                height: 1080
            }
        );
        for malformed in ["1920", "1920x", "x1080", "1920x1080x2", "-1x5", "a x b", ""] {
            assert_eq!(
                malformed.parse::<OutputSize>(),
                Err(SizeError::Malformed(malformed.to_owned())),
                "{malformed:?}"
            );
        }
        assert_eq!(
            "0x1080".parse::<OutputSize>(),
            Err(SizeError::Zero("0x1080".to_owned()))
        );
        assert_eq!(
            "1920x0".parse::<OutputSize>(),
            Err(SizeError::Zero("1920x0".to_owned()))
        );
    }

    #[test]
    fn refuses_more_sizes_than_outputs() {
        let sizes = [size("1920x1080"), size("3840x2160")];
        assert_eq!(
            Outputs::plain(1).sized(&sizes),
            Err(TooManySizes { sizes: 2, count: 1 })
        );
        let two = Outputs::plain(2).sized(&sizes).unwrap();
        assert_eq!(two.size(0), sizes[0]);
        assert_eq!(two.size(1), sizes[1]);
        assert_eq!(Outputs::plain(2).size(1), OutputSize::DEFAULT);
    }

    #[test]
    fn lays_them_out_side_by_side_in_logical_pixels() {
        let sizes = [size("1920x1080"), size("3840x2160")];
        let plain = Outputs::plain(3).sized(&sizes).unwrap();
        assert_eq!(
            plain.layout(),
            [
                (sizes[0], (0, 0)),
                (sizes[1], (1920, 0)),
                (OutputSize::DEFAULT, (5760, 0)),
            ]
        );
        let scaled = Outputs::new(2, Scale::TWO, OutputTransform::default())
            .sized(&sizes)
            .unwrap();
        assert_eq!(scaled.layout()[1].1, (960, 0));
        let turned = Outputs::new(2, Scale::ONE, Transform::Rotated90.into())
            .sized(&sizes)
            .unwrap();
        assert_eq!(turned.layout()[1].1, (1080, 0));
    }
}
