//! What the portal says, setting by setting, and how its values are read.

use zbus::Variant;

use crate::error::AppearanceError;
use crate::scheme::ColorScheme;

/// The desktop's appearance preferences, as the settings portal states
/// them. A setting the portal does not provide has its default: no
/// colour-scheme preference, no icon theme.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Preferences {
    /// `org.freedesktop.appearance` `color-scheme`.
    pub color_scheme: ColorScheme,
    /// The icon theme, by the two keys portals serve it under.
    pub icon_themes: IconThemes,
}

/// The icon theme the desktop uses, by the two keys the settings portals
/// serve it under; `None` where a portal provides no such key, or an empty
/// name. Which one wins is the caller's decision.
///
/// Neither key is a dependency on the desktop it is named after: they are
/// the names under which the portals publish the setting. KDE's portal
/// serves its `kdeglobals` as `org.kde.kdeglobals.*`, the GNOME and GTK
/// portals serve the `org.gnome.desktop.interface` schema, and any portal
/// (or a desktop's own settings tool) may serve either.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct IconThemes {
    /// `org.kde.kdeglobals.Icons` `Theme`.
    pub kde: Option<String>,
    /// `org.gnome.desktop.interface` `icon-theme`.
    pub gnome: Option<String>,
}

/// A setting this crate reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    ColorScheme,
    KdeIconTheme,
    GnomeIconTheme,
}

impl Key {
    /// Every setting read, in the order they are asked for.
    pub(crate) const ALL: [Self; 3] = [Self::ColorScheme, Self::KdeIconTheme, Self::GnomeIconTheme];

    /// The setting's namespace.
    pub(crate) fn namespace(self) -> &'static str {
        match self {
            Self::ColorScheme => "org.freedesktop.appearance",
            Self::KdeIconTheme => "org.kde.kdeglobals.Icons",
            Self::GnomeIconTheme => "org.gnome.desktop.interface",
        }
    }

    /// The setting's key in its namespace.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::ColorScheme => "color-scheme",
            Self::KdeIconTheme => "Theme",
            Self::GnomeIconTheme => "icon-theme",
        }
    }

    /// The namespace and key, for messages.
    fn label(self) -> &'static str {
        match self {
            Self::ColorScheme => "org.freedesktop.appearance color-scheme",
            Self::KdeIconTheme => "org.kde.kdeglobals.Icons Theme",
            Self::GnomeIconTheme => "org.gnome.desktop.interface icon-theme",
        }
    }

    /// The setting with this namespace and key, if it is one read.
    pub(crate) fn find(namespace: &str, key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|known| known.namespace() == namespace && known.name() == key)
    }
}

impl Preferences {
    /// Sets `key` from the portal's `value` (`None`: the portal does not
    /// provide it, so it has its default). Returns whether it changed.
    ///
    /// # Errors
    ///
    /// [`AppearanceError::UnexpectedValue`] when the value is of a type the
    /// setting cannot have; the preference is left as it was.
    pub(crate) fn set(
        &mut self,
        key: Key,
        value: Option<&Variant<'_>>,
    ) -> Result<bool, AppearanceError> {
        let value = value.map(peel);
        let changed = match key {
            Key::ColorScheme => {
                let scheme = value
                    .map(|value| match value {
                        Variant::U32(number) => Ok(ColorScheme::from_portal(*number)),
                        other => Err(unexpected(key, "u32", other)),
                    })
                    .transpose()?
                    .unwrap_or_default();
                replace(&mut self.color_scheme, scheme)
            }
            Key::KdeIconTheme => replace(&mut self.icon_themes.kde, theme_name(key, value)?),
            Key::GnomeIconTheme => replace(&mut self.icon_themes.gnome, theme_name(key, value)?),
        };
        Ok(changed)
    }
}

/// A theme name from a string value; an empty one is none.
fn theme_name(key: Key, value: Option<&Variant<'_>>) -> Result<Option<String>, AppearanceError> {
    match value {
        None => Ok(None),
        Some(Variant::Str(name)) => Ok((!name.is_empty()).then(|| name.as_str().to_owned())),
        Some(other) => Err(unexpected(key, "string", other)),
    }
}

/// The value inside any variants around it: the portal's older `Read`
/// method wraps its answer in one more.
fn peel<'v>(value: &'v Variant<'v>) -> &'v Variant<'v> {
    match value {
        Variant::Variant(inner) => peel(inner),
        other => other,
    }
}

fn unexpected(key: Key, expected: &'static str, found: &Variant<'_>) -> AppearanceError {
    AppearanceError::UnexpectedValue {
        setting: key.label(),
        expected,
        found: found.value_signature().to_string(),
    }
}

/// Sets `slot` to `value`; whether that changed it.
fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    let changed = *slot != value;
    *slot = value;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped(value: Variant<'static>) -> Variant<'static> {
        Variant::Variant(Box::new(Variant::Variant(Box::new(value))))
    }

    #[test]
    fn reads_each_setting_inside_any_variants() {
        let mut preferences = Preferences::default();
        assert!(
            preferences
                .set(Key::ColorScheme, Some(&wrapped(Variant::U32(1))))
                .unwrap()
        );
        assert!(
            preferences
                .set(Key::KdeIconTheme, Some(&Variant::from("breeze")))
                .unwrap()
        );
        assert!(
            preferences
                .set(
                    Key::GnomeIconTheme,
                    Some(&wrapped(Variant::from("Adwaita")))
                )
                .unwrap()
        );
        assert_eq!(
            preferences,
            Preferences {
                color_scheme: ColorScheme::PreferDark,
                icon_themes: IconThemes {
                    kde: Some("breeze".into()),
                    gnome: Some("Adwaita".into()),
                },
            }
        );
        // The same value again is no change.
        let same = preferences.set(Key::KdeIconTheme, Some(&Variant::from("breeze")));
        assert!(!same.unwrap());
    }

    #[test]
    fn a_missing_setting_or_an_empty_name_is_the_default() {
        let mut preferences = Preferences::default();
        preferences
            .set(Key::KdeIconTheme, Some(&Variant::from("breeze")))
            .unwrap();
        assert!(preferences.set(Key::KdeIconTheme, None).unwrap());
        preferences
            .set(Key::GnomeIconTheme, Some(&Variant::from("")))
            .unwrap();
        preferences.set(Key::ColorScheme, None).unwrap();
        assert_eq!(preferences, Preferences::default());
    }

    #[test]
    fn rejects_a_value_of_the_wrong_type_and_keeps_the_last() {
        let mut preferences = Preferences::default();
        preferences
            .set(Key::KdeIconTheme, Some(&Variant::from("breeze")))
            .unwrap();
        let error = preferences
            .set(Key::KdeIconTheme, Some(&Variant::U32(3)))
            .unwrap_err();
        assert!(matches!(
            error,
            AppearanceError::UnexpectedValue { setting: "org.kde.kdeglobals.Icons Theme", expected: "string", found }
                if found == "u"
        ));
        let error = preferences
            .set(Key::ColorScheme, Some(&Variant::from("dark")))
            .unwrap_err();
        assert!(error.to_string().contains("color-scheme is a s, not a u32"));
        assert_eq!(preferences.icon_themes.kde.as_deref(), Some("breeze"));
    }

    #[test]
    fn knows_its_settings_by_namespace_and_key() {
        for key in Key::ALL {
            assert_eq!(Key::find(key.namespace(), key.name()), Some(key));
        }
        assert_eq!(
            Key::find("org.freedesktop.appearance", "accent-color"),
            None
        );
        assert_eq!(Key::find("org.kde.kdeglobals.General", "Theme"), None);
    }
}
