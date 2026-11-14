//! The shell's look: the one set of rinch theme properties every component
//! uses.
//!
//! It is rinch's theme with its defaults (palette, radius, spacing, type),
//! dark or light. Components style themselves with the variables it
//! generates (`--rinch-color-body`, `--rinch-color-text`,
//! `--rinch-spacing-*`, `--rinch-font-*`, ...) and write no colour or metric
//! of their own.

use rinch::prelude::ThemeProviderProps;

use crate::scheme::Scheme;

/// The desktop's background where the wallpaper is given no colour: the
/// look's surface colour (`--rinch-color-default`), dark or light, set
/// apart from the body colour the panel is drawn in.
pub const DESKTOP_BACKGROUND: &str = "var(--rinch-color-default)";

/// The theme properties of the shell's look in `scheme`.
#[must_use]
pub fn props(scheme: Scheme) -> ThemeProviderProps {
    ThemeProviderProps {
        dark_mode: scheme == Scheme::Dark,
        ..ThemeProviderProps::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_rinchs_default_theme_dark_or_light() {
        let dark = props(Scheme::Dark);
        assert!(dark.dark_mode);
        let light = props(Scheme::Light);
        assert!(!light.dark_mode);
        // Apart from dark mode, rinch's defaults (the props have no
        // `PartialEq`; their `Debug` shows every field).
        let undarkened = ThemeProviderProps {
            dark_mode: false,
            ..dark
        };
        let default = format!("{:?}", ThemeProviderProps::default());
        assert_eq!(format!("{undarkened:?}"), default);
        assert_eq!(format!("{light:?}"), default);
    }
}
