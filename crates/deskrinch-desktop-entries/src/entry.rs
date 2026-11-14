//! The model of one desktop entry, and parsing it from a file's bytes.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use deskrinch_keyfile::{Group, KeyFile, Locale, Localized, LocalizedString};

use crate::deviation::Deviation;
use crate::error::ParseError;
use crate::icon::{self, Icon};
use crate::translated::{self, Translated};

/// The group every desktop entry file starts with.
const MAIN_GROUP: &str = "Desktop Entry";

/// What a missing `Name` reads as, as in GLib.
const UNNAMED: &str = "Unnamed";

/// A desktop-file ID: the file's path below an `applications` directory with
/// `/` replaced by `-`, such as `org.kde.konsole.desktop` or
/// `kde-konsole.desktop` for `applications/kde/konsole.desktop`.
///
/// Entries with the same ID override each other; see [`crate::Catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DesktopId(String);

impl DesktopId {
    /// The ID of the file at `relative`, a path below an `applications`
    /// directory. `None` unless it is a plain relative path to a
    /// `.desktop` file with UTF-8 components.
    #[must_use]
    pub fn from_relative_path(relative: &Path) -> Option<Self> {
        let parts = relative
            .components()
            .map(|component| match component {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Self::new(parts.join("-"))
    }

    /// An ID from its text: it must end in `.desktop` after a non-empty
    /// name, and contain no `/`.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Option<Self> {
        let id = id.into();
        let stem = id.strip_suffix(".desktop")?;
        (!stem.is_empty() && !id.contains('/')).then_some(Self(id))
    }

    /// The ID's text, `….desktop`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The ID without `.desktop`: the application ID, which is also the
    /// D-Bus name of a `DBusActivatable` application.
    #[must_use]
    pub fn app_id(&self) -> &str {
        self.0.strip_suffix(".desktop").unwrap_or(&self.0)
    }
}

impl fmt::Display for DesktopId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A parsed desktop entry, its translated keys resolved for the locales it
/// was parsed for ([`DesktopEntry::parse`]).
///
/// `Icon` is read as an [`Icon`]: a name or an absolute path. Looking it up
/// in an icon theme is not this crate's job.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DesktopEntry {
    /// The desktop-file ID.
    pub id: DesktopId,
    /// The file the entry was read from (what `%k` expands to).
    pub path: PathBuf,
    /// The entry's type and the keys only that type has.
    pub kind: EntryKind,
    /// `Name`; `Unnamed` when the file has none.
    pub name: Translated<String>,
    /// `GenericName`, such as "Web Browser".
    pub generic_name: Option<Translated<String>>,
    /// `Comment`, a tooltip.
    pub comment: Option<String>,
    /// `Icon`: a name or an absolute path, as GLib reads it (see
    /// [`Icon`]); not looked up.
    pub icon: Option<Icon>,
    /// `Hidden`: the entry is deleted, and hides entries of the same ID
    /// further down the search path.
    pub hidden: bool,
    /// `NoDisplay`: the application exists (it may handle MIME types, and
    /// its windows may be matched to it) but is not shown in menus.
    pub no_display: bool,
    /// `OnlyShowIn`: the desktops to show the entry in; empty means all.
    pub only_show_in: Vec<String>,
    /// `NotShowIn`: the desktops not to show the entry in.
    pub not_show_in: Vec<String>,
    /// `Categories`.
    pub categories: Vec<String>,
    /// `Keywords`, for searching.
    pub keywords: Option<Translated<Vec<String>>>,
    /// How what was read of the file for the session's locales deviates from
    /// the specification, in the order found: the translations for other
    /// locales are not read (see [`DesktopEntry::parse`]). Empty for a
    /// conforming file.
    pub deviations: Vec<Deviation>,
}

/// The type of an entry, with the keys specific to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// `Type=Application`.
    Application(Application),
    /// `Type=Link`.
    Link {
        /// `URL`.
        url: String,
    },
    /// `Type=Directory`, a menu directory.
    Directory,
}

/// The keys of an application entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Application {
    /// `Exec`, unescaped as a string but with its quoting and field codes
    /// intact; see [`crate::exec`]. Absent when `DBusActivatable`, or when
    /// the file leaves it out (a [`Deviation::MissingKey`]).
    pub exec: Option<String>,
    /// `TryExec`: a program that must exist for the entry to be shown.
    pub try_exec: Option<String>,
    /// `Path`: the working directory to run the program in.
    pub working_dir: Option<PathBuf>,
    /// `Terminal`: the program runs in a terminal emulator.
    pub terminal: bool,
    /// `DBusActivatable`: launch over D-Bus (`org.freedesktop.Application`)
    /// rather than by `Exec`.
    pub dbus_activatable: bool,
    /// The `Actions` that have a `[Desktop Action …]` group, in `Actions`
    /// order.
    pub actions: Vec<Action>,
    /// `MimeType`.
    pub mime_types: Vec<String>,
    /// `StartupWMClass`: the window class or app ID its windows carry.
    pub startup_wm_class: Option<String>,
    /// `StartupNotify`.
    pub startup_notify: bool,
}

/// An additional action of an application, from a `[Desktop Action id]`
/// group.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Action {
    /// The action's identifier, as listed in `Actions`.
    pub id: String,
    /// `Name`; `Unnamed` when the group has none.
    pub name: String,
    /// `Icon`, read as the entry's is. GLib does not read actions' icons;
    /// the same rules apply here.
    pub icon: Option<Icon>,
    /// `Exec`. Absent when the application is `DBusActivatable`, or when
    /// the group leaves it out.
    pub exec: Option<String>,
}

impl DesktopEntry {
    /// Parses the contents of the file at `path`, whose ID is `id`, for a
    /// session whose preferred locales are `locales` (best first): each
    /// translated key keeps its value for them, chosen by the
    /// specification's matching, and the translations for other locales are
    /// skipped as the file is read
    /// ([`KeyFile::parse_for_locales`](deskrinch_keyfile::KeyFile::parse_for_locales)).
    ///
    /// The file is read as GLib reads it: what breaks the specification but
    /// GLib accepts is accepted and listed in
    /// [`deviations`](DesktopEntry::deviations). The entry, deviations
    /// included, is exactly what parsing the file with the skipped lines
    /// replaced by empty lines gives: a deviation only a skipped line would
    /// show is not listed, and a `Name` with only translations into other
    /// locales is a missing `Name`.
    ///
    /// # Errors
    ///
    /// [`ParseError`] for what GLib also refuses: a malformed line, a key
    /// before the first group, a first group other than `[Desktop Entry]`,
    /// a missing or unknown `Type`, or a link without `URL`.
    pub fn parse(
        id: DesktopId,
        path: PathBuf,
        contents: impl AsRef<[u8]>,
        locales: &[Locale],
    ) -> Result<Self, ParseError> {
        let mut deviations = Vec::new();
        let file = KeyFile::parse_for_locales(contents.as_ref(), locales, &mut deviations)?;
        if file.first_group() != Some(MAIN_GROUP) {
            return Err(ParseError::MissingDesktopEntryGroup);
        }
        let main = file
            .group(MAIN_GROUP)
            .ok_or(ParseError::MissingDesktopEntryGroup)?;
        let d = &mut deviations;
        let kind = match main
            .string("Type", d)
            .ok_or(ParseError::MissingKey { key: "Type" })?
            .as_str()
        {
            "Application" => EntryKind::Application(parse_application(&file, main, locales, d)),
            "Link" => EntryKind::Link {
                url: main
                    .string("URL", d)
                    .ok_or(ParseError::MissingKey { key: "URL" })?,
            },
            "Directory" => EntryKind::Directory,
            other => {
                return Err(ParseError::UnknownType {
                    value: other.into(),
                });
            }
        };
        Ok(Self {
            id,
            path,
            kind,
            name: Translated::resolve(name(main, d), locales),
            generic_name: main
                .localized("GenericName", d)
                .map(|value| Translated::resolve(value, locales)),
            comment: main
                .localized("Comment", d)
                .map(|value| translated::session_value(value, locales)),
            icon: icon::read(main, d).map(|value| translated::session_value(value, locales)),
            hidden: main.boolean("Hidden", d),
            no_display: main.boolean("NoDisplay", d),
            only_show_in: main.list("OnlyShowIn", d),
            not_show_in: main.list("NotShowIn", d),
            categories: main.list("Categories", d),
            keywords: main
                .localized_list("Keywords", d)
                .map(|value| Translated::resolve(value, locales)),
            deviations,
        })
    }

    /// The application keys, if this is an application.
    #[must_use]
    pub fn application(&self) -> Option<&Application> {
        match &self.kind {
            EntryKind::Application(app) => Some(app),
            EntryKind::Link { .. } | EntryKind::Directory => None,
        }
    }
}

/// A group's `Name`: as written, or GLib's `Unnamed` (keeping any
/// translations) when it has no unlocalised value.
fn name(group: &Group, deviations: &mut Vec<Deviation>) -> LocalizedString {
    if let Some(name) = group.localized("Name", deviations) {
        return name;
    }
    let translations = group.translations("Name", deviations);
    if translations.is_empty() {
        deviations.push(Deviation::MissingKey {
            group: group.name().into(),
            key: "Name",
        });
    }
    Localized {
        default: UNNAMED.into(),
        translations,
    }
}

/// `Exec`, noting its absence unless D-Bus activation makes it optional.
fn read_exec(
    group: &Group,
    dbus_activatable: bool,
    deviations: &mut Vec<Deviation>,
) -> Option<String> {
    let exec = group.string("Exec", deviations);
    if exec.is_none() && !dbus_activatable {
        deviations.push(Deviation::MissingKey {
            group: group.name().into(),
            key: "Exec",
        });
    }
    exec
}

fn parse_application(
    file: &KeyFile,
    main: &Group,
    locales: &[Locale],
    d: &mut Vec<Deviation>,
) -> Application {
    let dbus_activatable = main.boolean("DBusActivatable", d);
    let exec = read_exec(main, dbus_activatable, d);
    let mut actions = Vec::new();
    for id in main.list("Actions", d) {
        // A group that is not listed is ignored, silently: the
        // specification allows it.
        let Some(group) = file.group(&format!("Desktop Action {id}")) else {
            d.push(Deviation::ActionWithoutGroup { action: id });
            continue;
        };
        actions.push(Action {
            name: translated::session_value(name(group, d), locales),
            icon: icon::read(group, d).map(|value| translated::session_value(value, locales)),
            exec: read_exec(group, dbus_activatable, d),
            id,
        });
    }
    Application {
        exec,
        try_exec: main.string("TryExec", d),
        working_dir: main.string("Path", d).map(PathBuf::from),
        terminal: main.boolean("Terminal", d),
        dbus_activatable,
        actions,
        mime_types: main.list("MimeType", d),
        startup_wm_class: main.string("StartupWMClass", d),
        startup_notify: main.boolean("StartupNotify", d),
    }
}

#[cfg(test)]
#[path = "entry_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "leniency_tests.rs"]
mod leniency_tests;
