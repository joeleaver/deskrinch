//! `[general]`: settings every shell component shares.
//!
//! ```toml
//! [general]
//! color-scheme = "system"   # "system", "dark" or "light"
//! icon-theme = "system"     # "system", or an icon theme's name
//! # terminal = ["foot", "--"]  # unset: xdg-terminal-exec
//! ```
//!
//! Every key is optional; the values above are the defaults. A value of the
//! wrong type, a colour scheme that is none of the three, an empty icon
//! theme name, or a `terminal` with no program is a diagnostic with its file, line and column, and that
//! file's `[general]` is not used.
//!
//! - `color-scheme`: whether the shell is dark or light. `"system"` follows
//!   the desktop's preference, from the settings portal (the
//!   `org.freedesktop.appearance` `color-scheme` that the desktop's
//!   appearance settings write), and changes when it does; where there is no
//!   portal, or it states no preference, the shell is dark. `"dark"` and
//!   `"light"` set it whatever the desktop prefers.
//! - `icon-theme`: the icon theme the shell's icons come from, by the name
//!   of its directory (`breeze`, `Adwaita`, `Papirus-Dark`). `"system"`
//!   follows the desktop's, from the settings portal, and changes when it
//!   does: the portal's `org.kde.kdeglobals.Icons` `Theme`, else its
//!   `org.gnome.desktop.interface` `icon-theme`. Those are the keys portals
//!   serve the icon theme under, whatever the desktop: KDE's portal serves
//!   the first, the GNOME and GTK portals the second, and nothing asks
//!   which desktop runs. A portal theme that is not installed is passed
//!   over; with none left, the first installed of `breeze` and `Adwaita` is
//!   used, else `hicolor`, the theme every icon theme falls back to. A
//!   theme named here that is not installed is reported, and the shell goes
//!   on as with `"system"`. (A theme whose directory is called `system`
//!   cannot be named.) The resolution is `deskrinch_theme`'s.
//! - `terminal`: the terminal emulator that applications which run in a
//!   terminal (`Terminal=true` in their desktop entry) are started in, by
//!   the launcher and anything else of the shell's that starts
//!   applications: its program and arguments, as a list, to which the
//!   application's command line is appended (`["foot", "--"]`,
//!   `["konsole", "-e"]`, `["kitty"]`). Unset, it is `xdg-terminal-exec`,
//!   the freedesktop way to run a command in the user's preferred terminal,
//!   when that is installed. With neither, such an application does not
//!   start, and the error says to set this ([`terminal`], [`NoTerminal`]).
//!   There is no list of known terminals to guess from.

use std::fmt;

use serde::{Deserialize, Serialize};

mod terminal;

pub use terminal::{EmptyTerminal, NoTerminal, TerminalCommand, terminal};

/// The `[general]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct GeneralSettings {
    /// `color-scheme`: whether the shell is dark or light.
    pub color_scheme: ColorSchemeSetting,
    /// `icon-theme`: the icon theme the shell's icons come from.
    pub icon_theme: IconThemeSetting,
    /// `terminal`: the command line terminal applications run in; `None`:
    /// `xdg-terminal-exec`, when installed.
    pub terminal: Option<TerminalCommand>,
}

/// `icon-theme`: the icon theme the shell's icons come from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum IconThemeSetting {
    /// `"system"`, the default: the desktop's icon theme.
    #[default]
    System,
    /// The theme of this name: never empty, and never `"system"`.
    Named(String),
}

impl IconThemeSetting {
    /// The keyword for the desktop's icon theme.
    pub const SYSTEM: &'static str = "system";
}

impl TryFrom<String> for IconThemeSetting {
    type Error = EmptyIconTheme;

    fn try_from(name: String) -> Result<Self, EmptyIconTheme> {
        match name.as_str() {
            "" => Err(EmptyIconTheme),
            Self::SYSTEM => Ok(Self::System),
            _ => Ok(Self::Named(name)),
        }
    }
}

impl From<IconThemeSetting> for String {
    fn from(setting: IconThemeSetting) -> Self {
        match setting {
            IconThemeSetting::System => IconThemeSetting::SYSTEM.to_owned(),
            IconThemeSetting::Named(name) => name,
        }
    }
}

/// An `icon-theme` that is an empty string: no theme has that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyIconTheme;

impl fmt::Display for EmptyIconTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an icon theme's name cannot be empty; \"system\" follows the desktop's")
    }
}

impl std::error::Error for EmptyIconTheme {}

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

    #[test]
    fn reads_the_icon_theme() {
        assert_eq!(resolve("").settings.icon_theme, IconThemeSetting::System);
        for (text, setting) in [
            ("system", IconThemeSetting::System),
            ("breeze", IconThemeSetting::Named("breeze".into())),
            (
                "Papirus Dark",
                IconThemeSetting::Named("Papirus Dark".into()),
            ),
        ] {
            let resolved = resolve(&format!("[general]\nicon-theme = \"{text}\"\n"));
            assert_eq!(resolved.settings.icon_theme, setting, "{text}");
            assert!(
                resolved.diagnostics.is_empty(),
                "{:?}",
                resolved.diagnostics
            );
        }
    }

    #[test]
    fn rejects_an_empty_icon_theme_with_its_location() {
        let resolved = resolve("[general]\ncolor-scheme = \"light\"\nicon-theme = \"\"\n");
        assert_eq!(resolved.settings, GeneralSettings::default());
        let [diagnostic] = resolved.diagnostics.as_slice() else {
            panic!("{:?}", resolved.diagnostics);
        };
        assert!(matches!(diagnostic.problem, Problem::Invalid { .. }));
        assert_eq!(diagnostic.location.map(|l| l.line), Some(3));
    }

    #[test]
    fn writes_the_icon_theme_as_it_reads_it() {
        for setting in [
            IconThemeSetting::System,
            IconThemeSetting::Named("breeze".into()),
        ] {
            let text = String::from(setting.clone());
            assert_eq!(IconThemeSetting::try_from(text), Ok(setting));
        }
        assert_eq!(
            IconThemeSetting::try_from(String::new()),
            Err(EmptyIconTheme)
        );
    }

    #[test]
    fn reads_the_terminal() {
        assert_eq!(resolve("").settings.terminal, None);
        let resolved = resolve("[general]\nterminal = [\"foot\", \"--\"]\n");
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let terminal = resolved.settings.terminal.unwrap();
        assert_eq!(terminal.words(), ["foot", "--"]);
    }

    #[test]
    fn rejects_a_terminal_with_no_program_with_its_location() {
        for text in [
            "terminal = []",
            "terminal = [\"\", \"-e\"]",
            "terminal = \"foot\"",
        ] {
            let resolved = resolve(&format!("[general]\ncolor-scheme = \"light\"\n{text}\n"));
            assert_eq!(resolved.settings, GeneralSettings::default(), "{text}");
            let [diagnostic] = resolved.diagnostics.as_slice() else {
                panic!("{text}: {:?}", resolved.diagnostics);
            };
            assert!(
                matches!(diagnostic.problem, Problem::Invalid { .. }),
                "{text}"
            );
            assert_eq!(diagnostic.location.map(|l| l.line), Some(3), "{text}");
        }
    }
}
