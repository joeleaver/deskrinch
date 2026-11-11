//! The colour scheme, as the portal writes it.

/// The user's colour-scheme preference: the settings portal's
/// `org.freedesktop.appearance` `color-scheme`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    /// No preference (the portal's `0`), and any value the portal
    /// specification does not define, which it says to read as `0`. Also
    /// what a portal without the key states.
    #[default]
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
}
