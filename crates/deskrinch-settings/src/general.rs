//! `[general]`: settings every shell component shares.
//!
//! ```toml
//! [general]
//! color-scheme = "system"   # "system", "dark" or "light"
//! ```
//!
//! Every key is optional; the value above is the default. A value of the
//! wrong type, or a colour scheme that is none of the three, is a diagnostic
//! with its file, line and column, and that file's `[general]` is not used.
//!
//! - `color-scheme`: whether the shell is dark or light. `"system"` follows
//!   the desktop's preference, from the settings portal (the
//!   `org.freedesktop.appearance` `color-scheme` that the desktop's
//!   appearance settings write), and changes when it does; where there is no
//!   portal, or it states no preference, the shell is dark. `"dark"` and
//!   `"light"` set it whatever the desktop prefers.

use serde::{Deserialize, Serialize};

/// The `[general]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GeneralSettings {
    /// `color-scheme`: whether the shell is dark or light.
    pub color_scheme: ColorSchemeSetting,
}

/// `color-scheme`: whether the shell is dark or light.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorSchemeSetting {
    /// `"system"`, the default: the desktop's preference, dark where it
    /// states none.
    #[default]
    System,
    /// `"dark"`.
    Dark,
    /// `"light"`.
    Light,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section;
    use deskrinch_config::{Problem, Resolved, Snapshot};
    use std::path::PathBuf;

    fn resolve(text: &str) -> Resolved<GeneralSettings> {
        let path = PathBuf::from("/home/u/.config/deskrinch/config.toml");
        let snapshot = Snapshot::from_texts([(path, Some(text.to_owned()))]);
        section::<GeneralSettings>().unwrap().resolve(&snapshot)
    }

    #[test]
    fn follows_the_system_by_default() {
        let resolved = resolve("");
        assert_eq!(resolved.settings.color_scheme, ColorSchemeSetting::System);
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
    }

    #[test]
    fn reads_each_colour_scheme() {
        for (text, scheme) in [
            ("system", ColorSchemeSetting::System),
            ("dark", ColorSchemeSetting::Dark),
            ("light", ColorSchemeSetting::Light),
        ] {
            let resolved = resolve(&format!("[general]\ncolor-scheme = \"{text}\"\n"));
            assert_eq!(resolved.settings.color_scheme, scheme, "{text}");
            assert!(
                resolved.diagnostics.is_empty(),
                "{:?}",
                resolved.diagnostics
            );
        }
    }

    #[test]
    fn rejects_another_scheme_with_its_location() {
        let resolved = resolve("[general]\ncolor-scheme = \"blue\"\n");
        assert_eq!(resolved.settings, GeneralSettings::default());
        let [diagnostic] = resolved.diagnostics.as_slice() else {
            panic!("{:?}", resolved.diagnostics);
        };
        assert!(matches!(diagnostic.problem, Problem::Invalid { .. }));
        assert_eq!(diagnostic.location.map(|l| l.line), Some(2));
    }
}
