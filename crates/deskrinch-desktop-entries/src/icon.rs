//! An entry's `Icon`, read as GLib reads it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use deskrinch_keyfile::{Group, Localized};

use crate::deviation::Deviation;

/// The extensions GLib strips from an icon name, exactly as written.
const IMAGE_EXTENSIONS: [&str; 3] = [".png", ".xpm", ".svg"];

/// What an `Icon` key names: an icon to look up in the icon theme by name,
/// or a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    /// An icon name, such as `org.gnome.Nautilus`, for an icon-theme
    /// lookup.
    Name(String),
    /// An absolute path to the icon's file.
    Path(PathBuf),
}

impl Icon {
    /// The icon as `Exec`'s `%i` passes it after `--icon`: the name as
    /// read (with an extension stripped), or the path.
    #[must_use]
    pub fn as_os_str(&self) -> &OsStr {
        match self {
            Self::Name(name) => OsStr::new(name),
            Self::Path(path) => path.as_os_str(),
        }
    }

    /// Reads one `Icon` value as GLib's `g_desktop_app_info_load_from_keyfile`
    /// does:
    ///
    /// - An absolute path is the file.
    /// - Anything else is a name, without a final `.png`, `.xpm` or `.svg`
    ///   (matched exactly, so `.PNG` stays): `firefox.png` is `firefox`,
    ///   while `org.gnome.Nautilus` stays whole. GLib calls this working
    ///   around "a common mistake in desktop files"; here it is a
    ///   [`Deviation::IconExtension`].
    /// - A relative path such as `icons/app.png` is a name too, as in GLib
    ///   (`icons/app`), which no icon theme has; it is a
    ///   [`Deviation::RelativeIconPath`].
    ///
    /// GLib makes an empty value, or one that is only an extension
    /// (`.png`), an icon named `""` that is never found; here that is no
    /// icon.
    fn read(value: &str, group: &Group, deviations: &mut Vec<Deviation>) -> Option<Self> {
        if Path::new(value).is_absolute() {
            return Some(Self::Path(value.into()));
        }
        let name = match IMAGE_EXTENSIONS
            .iter()
            .find_map(|extension| value.strip_suffix(extension))
        {
            Some(stripped) => {
                deviations.push(Deviation::IconExtension {
                    group: group.name().into(),
                    value: value.into(),
                });
                stripped
            }
            None => value,
        };
        if name.contains('/') {
            deviations.push(Deviation::RelativeIconPath {
                group: group.name().into(),
                value: value.into(),
            });
        }
        (!name.is_empty()).then(|| Self::Name(name.into()))
    }
}

/// A group's `Icon` and its translations, each read by [`Icon::read`]. No
/// icon when the unlocalised value is absent or empty; an empty
/// translation is dropped, so its locale falls back as for any key.
pub(crate) fn read(group: &Group, deviations: &mut Vec<Deviation>) -> Option<Localized<Icon>> {
    let value = group.localized("Icon", deviations)?;
    let default = Icon::read(&value.default, group, deviations)?;
    let mut icon = Localized::new(default);
    icon.translations = value
        .translations
        .iter()
        .filter_map(|(locale, value)| Some((locale.clone(), Icon::read(value, group, deviations)?)))
        .collect();
    Some(icon)
}

#[cfg(test)]
#[path = "icon_tests.rs"]
mod tests;
