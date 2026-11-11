//! Which icon theme: what the setting and the desktop's icon theme make,
//! among the themes that are installed.

use deskrinch_appearance::IconThemes;
use deskrinch_icons::HICOLOR;
use deskrinch_settings::general::IconThemeSetting;

/// The themes tried, in order, when neither the setting nor the desktop
/// names one that is installed: the KDE and GNOME defaults, the two most
/// often installed. `hicolor` comes after them.
pub const DEFAULT_ICON_THEMES: [&str; 2] = ["breeze", "Adwaita"];

/// The icon theme [`resolve_icon_theme`] chose, and the themes it was asked
/// for that are not installed, for the caller to report.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedIconTheme {
    /// The theme's name, to look icons up in.
    pub name: String,
    /// The themes named by the setting or the portal that were passed over
    /// because they are not installed, in the order they were tried.
    pub not_installed: Vec<String>,
}

/// The icon theme for the `[general]` `icon-theme` `setting` and the icon
/// themes the settings portal states (`IconThemes::default()` when there is
/// no portal), where `installed` says whether a theme of that name is
/// installed (`deskrinch_icons::SearchPath::has_theme`):
///
/// 1. a theme the setting names, if it is installed;
/// 2. the portal's `org.kde.kdeglobals.Icons` `Theme`, if installed;
/// 3. the portal's `org.gnome.desktop.interface` `icon-theme`, if
///    installed;
/// 4. the first installed of [`DEFAULT_ICON_THEMES`];
/// 5. `hicolor`, installed or not: every lookup ends there anyway.
///
/// `"system"` starts at 2. A named theme that is not installed goes on from
/// 2 as `"system"` does, and is in
/// [`not_installed`](ResolvedIconTheme::not_installed), as is a portal's
/// theme that is not. Which desktop runs is never asked: the two portal
/// keys are tried in this order whoever serves them.
#[must_use]
pub fn resolve_icon_theme(
    setting: &IconThemeSetting,
    portal: &IconThemes,
    installed: impl Fn(&str) -> bool,
) -> ResolvedIconTheme {
    let named = match setting {
        IconThemeSetting::System => None,
        IconThemeSetting::Named(name) => Some(name.as_str()),
    };
    let asked = [named, portal.kde.as_deref(), portal.gnome.as_deref()];
    let mut not_installed: Vec<String> = Vec::new();
    for name in asked.into_iter().flatten() {
        if installed(name) {
            return ResolvedIconTheme {
                name: name.to_owned(),
                not_installed,
            };
        }
        if !not_installed.iter().any(|seen| seen == name) {
            not_installed.push(name.to_owned());
        }
    }
    let name = DEFAULT_ICON_THEMES
        .into_iter()
        .find(|name| installed(name))
        .unwrap_or(HICOLOR);
    ResolvedIconTheme {
        name: name.to_owned(),
        not_installed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> IconThemeSetting {
        IconThemeSetting::Named(name.to_owned())
    }

    fn portal(kde: Option<&str>, gnome: Option<&str>) -> IconThemes {
        IconThemes {
            kde: kde.map(str::to_owned),
            gnome: gnome.map(str::to_owned),
        }
    }

    /// `installed` for exactly `themes`.
    fn only<'a>(themes: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |name| themes.contains(&name)
    }

    fn resolve(
        setting: &IconThemeSetting,
        portal: &IconThemes,
        installed: &[&str],
    ) -> (String, Vec<String>) {
        let resolved = resolve_icon_theme(setting, portal, only(installed));
        (resolved.name, resolved.not_installed)
    }

    fn chose(name: &str, not_installed: &[&str]) -> (String, Vec<String>) {
        let not_installed = not_installed.iter().map(|&name| name.to_owned()).collect();
        (name.to_owned(), not_installed)
    }

    #[test]
    fn system_takes_the_kde_key_then_the_gnome_key() {
        let all = ["breeze", "Adwaita", "oxygen", "Papirus"];
        let system = IconThemeSetting::System;
        let both = portal(Some("oxygen"), Some("Papirus"));
        assert_eq!(resolve(&system, &both, &all), chose("oxygen", &[]));
        let gnome = portal(None, Some("Papirus"));
        assert_eq!(resolve(&system, &gnome, &all), chose("Papirus", &[]));
    }

    #[test]
    fn system_without_a_portal_theme_takes_the_defaults_then_hicolor() {
        let system = IconThemeSetting::System;
        let none = IconThemes::default();
        let resolved = |installed: &[&str]| resolve(&system, &none, installed);
        assert_eq!(resolved(&["Adwaita", "breeze"]), chose("breeze", &[]));
        assert_eq!(resolved(&["Adwaita"]), chose("Adwaita", &[]));
        assert_eq!(resolved(&["hicolor"]), chose("hicolor", &[]));
        // Nothing installed at all: `hicolor` still, where lookups end.
        assert_eq!(resolved(&[]), chose("hicolor", &[]));
    }

    #[test]
    fn a_portal_theme_that_is_not_installed_is_passed_over_and_reported() {
        let system = IconThemeSetting::System;
        let both = portal(Some("oxygen"), Some("Papirus"));
        assert_eq!(
            resolve(&system, &both, &["Papirus", "breeze"]),
            chose("Papirus", &["oxygen"])
        );
        assert_eq!(
            resolve(&system, &both, &["Adwaita"]),
            chose("Adwaita", &["oxygen", "Papirus"])
        );
    }

    #[test]
    fn a_named_theme_wins_over_the_portal() {
        let both = portal(Some("oxygen"), Some("Papirus"));
        let installed = ["oxygen", "Papirus", "Tela"];
        assert_eq!(
            resolve(&named("Tela"), &both, &installed),
            chose("Tela", &[])
        );
    }

    #[test]
    fn a_named_theme_that_is_not_installed_goes_on_as_system_does() {
        let both = portal(Some("oxygen"), None);
        assert_eq!(
            resolve(&named("Tela"), &both, &["oxygen", "breeze"]),
            chose("oxygen", &["Tela"])
        );
        let none = IconThemes::default();
        assert_eq!(
            resolve(&named("Tela"), &none, &["breeze"]),
            chose("breeze", &["Tela"])
        );
        // Named and also the portal's: tried, and reported, once.
        let same = portal(Some("Tela"), None);
        assert_eq!(
            resolve(&named("Tela"), &same, &[]),
            chose("hicolor", &["Tela"])
        );
    }
}
