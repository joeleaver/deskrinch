//! The scale a session's outputs have from the start.

use std::fmt;
use std::str::FromStr;

/// A positive, finite output scale: how many physical pixels a logical one
/// covers (`2`, `1.5`, ...).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Scale(f64);

impl Scale {
    /// Scale 1: what a compositor starts with unless told otherwise.
    pub(crate) const ONE: Self = Self(1.0);
    /// Scale 2: a high-density screen.
    pub(crate) const TWO: Self = Self(2.0);

    /// The scale as a number.
    pub(crate) fn get(self) -> f64 {
        self.0
    }

    /// The whole scale `wl_output` reports for it: rounded up.
    pub(crate) fn whole(self) -> i32 {
        let mut whole = 1;
        while f64::from(whole) < self.0 {
            whole += 1;
        }
        whole
    }

    /// Whether it is 1, the compositors' own default.
    pub(crate) fn is_one(self) -> bool {
        self == Self::ONE
    }
}

impl FromStr for Scale {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let value: f64 = text
            .parse()
            .map_err(|_| format!("{text:?} is not a number"))?;
        if value.is_finite() && value > 0.0 {
            Ok(Self(value))
        } else {
            Err(format!("{text} is not a positive scale"))
        }
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `2` rather than `2.0`, and `1.5` as it was given: what the
        // compositors' own configuration takes.
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_positive_decimals() {
        assert_eq!("2".parse::<Scale>(), Ok(Scale::TWO));
        assert_eq!("1.5".parse::<Scale>().unwrap().to_string(), "1.5");
        assert!("1".parse::<Scale>().unwrap().is_one());
        for bad in ["0", "-1", "inf", "NaN", "two", ""] {
            assert!(bad.parse::<Scale>().is_err(), "{bad}");
        }
    }

    #[test]
    fn rounds_up_to_a_whole_scale() {
        let whole = |text: &str| text.parse::<Scale>().unwrap().whole();
        assert_eq!(whole("1"), 1);
        assert_eq!(whole("1.25"), 2);
        assert_eq!(whole("2"), 2);
        assert_eq!(whole("0.5"), 1);
    }

    #[test]
    fn prints_as_the_compositors_read_it() {
        assert_eq!("2".parse::<Scale>().unwrap().to_string(), "2");
        assert_eq!("1.25".parse::<Scale>().unwrap().to_string(), "1.25");
    }
}
