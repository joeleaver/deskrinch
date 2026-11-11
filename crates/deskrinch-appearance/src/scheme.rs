//! The colour scheme, as the portal writes it.

use zbus::{OwnedVariant, Variant};

use crate::error::AppearanceError;

/// The user's colour-scheme preference: the settings portal's
/// `org.freedesktop.appearance` `color-scheme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    /// No preference (the portal's `0`), and any value the portal
    /// specification does not define, which it says to read as `0`.
    NoPreference,
    /// The user prefers a dark appearance (`1`).
    PreferDark,
    /// The user prefers a light appearance (`2`).
    PreferLight,
}

impl ColorScheme {
    /// The scheme the portal's number stands for.
    #[must_use]
    pub fn from_portal(value: u32) -> Self {
        match value {
            1 => Self::PreferDark,
            2 => Self::PreferLight,
            _ => Self::NoPreference,
        }
    }

    /// The scheme in a value the portal sent: a `u32`, possibly inside one
    /// or more variants (the portal's older `Read` method wraps its answer
    /// in one more).
    pub(crate) fn from_value(value: &Variant<'_>) -> Result<Self, AppearanceError> {
        match value {
            Variant::U32(number) => Ok(Self::from_portal(*number)),
            Variant::Variant(inner) => Self::from_value(inner),
            other => Err(AppearanceError::UnexpectedValue {
                found: other.value_signature().to_string(),
            }),
        }
    }

    /// [`from_value`](Self::from_value) for an owned value.
    pub(crate) fn from_owned(value: &OwnedVariant) -> Result<Self, AppearanceError> {
        Self::from_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_portals_numbers() {
        assert_eq!(ColorScheme::from_portal(0), ColorScheme::NoPreference);
        assert_eq!(ColorScheme::from_portal(1), ColorScheme::PreferDark);
        assert_eq!(ColorScheme::from_portal(2), ColorScheme::PreferLight);
    }

    #[test]
    fn reads_an_unknown_number_as_no_preference() {
        for unknown in [3, 7, u32::MAX] {
            assert_eq!(ColorScheme::from_portal(unknown), ColorScheme::NoPreference);
        }
    }

    #[test]
    fn unwraps_variants_and_rejects_other_types() {
        let wrapped = Variant::Variant(Box::new(Variant::Variant(Box::new(Variant::U32(1)))));
        assert_eq!(
            ColorScheme::from_value(&wrapped).unwrap(),
            ColorScheme::PreferDark
        );
        assert_eq!(
            ColorScheme::from_value(&Variant::U32(2)).unwrap(),
            ColorScheme::PreferLight
        );
        let error = ColorScheme::from_value(&Variant::from("dark")).unwrap_err();
        assert!(matches!(error, AppearanceError::UnexpectedValue { found } if found == "s"));
    }
}
