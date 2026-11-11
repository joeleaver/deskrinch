//! Dark or light: what the setting and the desktop's preference make.

use deskrinch_appearance::ColorScheme;
use deskrinch_settings::general::ColorSchemeSetting;

/// Whether the shell is dark or light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    /// Dark: light text on dark surfaces.
    Dark,
    /// Light: dark text on light surfaces.
    Light,
}

/// The scheme for the `[general]` `color-scheme` `setting` and what the
/// settings portal says (`None` when there is no portal on the bus):
///
/// - `"dark"` or `"light"` wins, whatever the desktop prefers;
/// - `"system"` follows the portal's preference;
/// - with no portal, or no preference from it, the shell is dark.
#[must_use]
pub fn resolve(setting: ColorSchemeSetting, portal: Option<ColorScheme>) -> Scheme {
    match (setting, portal) {
        (ColorSchemeSetting::Light, _)
        | (ColorSchemeSetting::System, Some(ColorScheme::PreferLight)) => Scheme::Light,
        (ColorSchemeSetting::Dark, _)
        | (
            ColorSchemeSetting::System,
            Some(ColorScheme::PreferDark | ColorScheme::NoPreference) | None,
        ) => Scheme::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_every_setting_and_portal_answer() {
        use ColorScheme::{NoPreference, PreferDark, PreferLight};
        use ColorSchemeSetting::{Dark, Light, System};
        let cases = [
            (System, None, Scheme::Dark),
            (System, Some(NoPreference), Scheme::Dark),
            (System, Some(PreferDark), Scheme::Dark),
            (System, Some(PreferLight), Scheme::Light),
            (Dark, None, Scheme::Dark),
            (Dark, Some(PreferLight), Scheme::Dark),
            (Light, None, Scheme::Light),
            (Light, Some(PreferDark), Scheme::Light),
            (Light, Some(NoPreference), Scheme::Light),
        ];
        for (setting, portal, scheme) in cases {
            assert_eq!(resolve(setting, portal), scheme, "{setting:?} {portal:?}");
        }
    }
}
