//! The transform a session's outputs have from the start.

use std::fmt;
use std::str::FromStr;

use deskrinch_test_wayland::Transform;

/// An output transform as `wl_output.transform` means it (rotations
/// counter-clockwise), named as the session's `--transform` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct OutputTransform(Transform);

/// Each transform's name on the command line, in `Transform::ALL`'s order.
const NAMES: [&str; 8] = [
    "normal",
    "90",
    "180",
    "270",
    "flipped",
    "flipped-90",
    "flipped-180",
    "flipped-270",
];

impl OutputTransform {
    /// The transform.
    pub(crate) fn get(self) -> Transform {
        self.0
    }

    /// Whether it is the identity, every compositor's default.
    pub(crate) fn is_normal(self) -> bool {
        self.0 == Transform::Normal
    }

    /// Its name for `kscreen-doctor`'s `rotation` (`KScreen` calls a quarter
    /// turn counter-clockwise "left").
    pub(crate) fn kscreen_name(self) -> &'static str {
        match self.0 {
            Transform::Normal => "none",
            Transform::Rotated90 => "left",
            Transform::Rotated180 => "inverted",
            Transform::Rotated270 => "right",
            Transform::Flipped => "flipped",
            Transform::Flipped90 => "flipped90",
            Transform::Flipped180 => "flipped180",
            Transform::Flipped270 => "flipped270",
        }
    }

    /// Its name for sway's `output … transform`, whose rotations are
    /// clockwise: sway's `90` is `wl_output`'s 270.
    pub(crate) fn sway_name(self) -> &'static str {
        match self.0 {
            Transform::Normal => "normal",
            Transform::Rotated90 => "270",
            Transform::Rotated180 => "180",
            Transform::Rotated270 => "90",
            Transform::Flipped => "flipped",
            Transform::Flipped90 => "flipped-270",
            Transform::Flipped180 => "flipped-180",
            Transform::Flipped270 => "flipped-90",
        }
    }
}

impl From<Transform> for OutputTransform {
    fn from(transform: Transform) -> Self {
        Self(transform)
    }
}

impl FromStr for OutputTransform {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        NAMES
            .iter()
            .position(|&name| name == text)
            .map(|index| Self(Transform::ALL[index]))
            .ok_or_else(|| format!("{text:?} is not one of {}", NAMES.join(", ")))
    }
}

impl fmt::Display for OutputTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let index = Transform::ALL
            .iter()
            .position(|&transform| transform == self.0)
            .unwrap_or_default();
        f.write_str(NAMES[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for name in NAMES {
            assert_eq!(name.parse::<OutputTransform>().unwrap().to_string(), name);
        }
        assert!("sideways".parse::<OutputTransform>().is_err());
        assert_eq!(
            "90".parse::<OutputTransform>().unwrap().get(),
            Transform::Rotated90
        );
    }

    /// As measured: sway's `transform 90` gave `wl_output` 270, and `KScreen`'s
    /// `left` gave 90.
    #[test]
    fn each_tool_gets_its_own_name() {
        let ninety = OutputTransform(Transform::Rotated90);
        assert_eq!(ninety.sway_name(), "270");
        assert_eq!(ninety.kscreen_name(), "left");
        assert_eq!(
            OutputTransform(Transform::Flipped90).sway_name(),
            "flipped-270"
        );
        assert!(OutputTransform::default().is_normal());
    }
}
