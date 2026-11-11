//! What a session's outputs are from the start: how many, at what scale,
//! turned how.

use std::fmt;

use super::{OutputTransform, Scale};

/// A session's outputs, side by side, each 1280×720.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Outputs {
    /// How many.
    pub(crate) count: u32,
    /// Their scale.
    pub(crate) scale: Scale,
    /// Their transform.
    pub(crate) transform: OutputTransform,
}

impl Outputs {
    /// `count` outputs at scale 1, untransformed.
    pub(crate) fn plain(count: u32) -> Self {
        Self {
            count,
            scale: Scale::ONE,
            transform: OutputTransform::default(),
        }
    }

    /// Whether they are as every compositor starts them.
    pub(crate) fn is_plain(self) -> bool {
        self.scale.is_one() && self.transform.is_normal()
    }
}

impl fmt::Display for Outputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} output(s) at scale {}", self.count, self.scale)?;
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

    #[test]
    fn says_what_they_are() {
        assert_eq!(Outputs::plain(2).to_string(), "2 output(s) at scale 1");
        let turned = Outputs {
            transform: Transform::Rotated90.into(),
            ..Outputs::plain(1)
        };
        assert_eq!(turned.to_string(), "1 output(s) at scale 1, transform 90");
        assert!(Outputs::plain(1).is_plain() && !turned.is_plain());
    }
}
