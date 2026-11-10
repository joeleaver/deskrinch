//! The model of one desktop entry, and parsing it from a file's text.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::error::ParseError;
use crate::keyfile::{Group, KeyFile};
use crate::locale::{LocalizedList, LocalizedString};

/// The group every desktop entry file has.
const MAIN_GROUP: &str = "Desktop Entry";

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

/// A parsed desktop entry.
///
/// `Icon` is kept as the string it is (a name or an absolute path); looking
/// it up in an icon theme is not this crate's job.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DesktopEntry {
    /// The desktop-file ID.
    pub id: DesktopId,
    /// The file the entry was read from (what `%k` expands to).
    pub path: PathBuf,
    /// The entry's type and the keys only that type has.
    pub kind: EntryKind,
    /// `Name`.
    pub name: LocalizedString,
    /// `GenericName`, such as "Web Browser".
    pub generic_name: Option<LocalizedString>,
    /// `Comment`, a tooltip.
    pub comment: Option<LocalizedString>,
    /// `Icon`, unresolved.
    pub icon: Option<LocalizedString>,
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
    pub keywords: Option<LocalizedList>,
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
    /// intact; see [`crate::exec`]. Absent only when `DBusActivatable`.
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
    /// `Name`.
    pub name: LocalizedString,
    /// `Icon`, unresolved.
    pub icon: Option<LocalizedString>,
    /// `Exec`. Absent only when the application is `DBusActivatable`.
    pub exec: Option<String>,
}

impl DesktopEntry {
    /// Parses the text of the file at `path`, whose ID is `id`.
    ///
    /// # Errors
    ///
    /// [`ParseError`] when the file is not a valid desktop entry: malformed
    /// syntax, a missing required key (`Type`, `Name`, an application's
    /// `Exec` unless `DBusActivatable`, a link's `URL`), an invalid
    /// boolean or an unknown `Type`.
    pub fn parse(id: DesktopId, path: PathBuf, text: &str) -> Result<Self, ParseError> {
        let file = KeyFile::parse(text)?;
        let main = file
            .group(MAIN_GROUP)
            .ok_or(ParseError::MissingDesktopEntryGroup)?;
        let kind = match main
            .string("Type")
            .ok_or(ParseError::MissingKey { key: "Type" })?
            .as_str()
        {
            "Application" => EntryKind::Application(parse_application(&file, main)?),
            "Link" => EntryKind::Link {
                url: main
                    .string("URL")
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
            name: main
                .localized("Name")
                .ok_or(ParseError::MissingKey { key: "Name" })?,
            generic_name: main.localized("GenericName"),
            comment: main.localized("Comment"),
            icon: main.localized("Icon"),
            hidden: main.boolean("Hidden")?,
            no_display: main.boolean("NoDisplay")?,
            only_show_in: main.list("OnlyShowIn"),
            not_show_in: main.list("NotShowIn"),
            categories: main.list("Categories"),
            keywords: main.localized_list("Keywords"),
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

fn parse_application(file: &KeyFile, main: &Group) -> Result<Application, ParseError> {
    let dbus_activatable = main.boolean("DBusActivatable")?;
    let exec = main.string("Exec");
    if exec.is_none() && !dbus_activatable {
        return Err(ParseError::MissingKey { key: "Exec" });
    }
    let actions = main
        .list("Actions")
        .into_iter()
        .filter_map(|id| {
            // An action listed without its group is ignored, as is a group
            // that is not listed.
            let group = file.group(&format!("Desktop Action {id}"))?;
            Some(parse_action(id, group, dbus_activatable))
        })
        .collect::<Result<_, _>>()?;
    Ok(Application {
        exec,
        try_exec: main.string("TryExec"),
        working_dir: main.string("Path").map(PathBuf::from),
        terminal: main.boolean("Terminal")?,
        dbus_activatable,
        actions,
        mime_types: main.list("MimeType"),
        startup_wm_class: main.string("StartupWMClass"),
        startup_notify: main.boolean("StartupNotify")?,
    })
}

fn parse_action(id: String, group: &Group, dbus_activatable: bool) -> Result<Action, ParseError> {
    let exec = group.string("Exec");
    if exec.is_none() && !dbus_activatable {
        return Err(ParseError::MissingKey { key: "Exec" });
    }
    Ok(Action {
        id,
        name: group
            .localized("Name")
            .ok_or(ParseError::MissingKey { key: "Name" })?,
        icon: group.localized("Icon"),
        exec,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::Locale;

    fn parse(text: &str) -> Result<DesktopEntry, ParseError> {
        let id = DesktopId::new("org.example.App.desktop").unwrap();
        DesktopEntry::parse(id, "/x/org.example.App.desktop".into(), text)
    }

    #[test]
    fn ids_come_from_the_relative_path() {
        let id = |p: &str| DesktopId::from_relative_path(Path::new(p)).map(|id| id.0);
        assert_eq!(
            id("kde/konsole.desktop").as_deref(),
            Some("kde-konsole.desktop")
        );
        assert_eq!(id("a.desktop").as_deref(), Some("a.desktop"));
        assert_eq!(id("../a.desktop"), None);
        assert_eq!(id("/a.desktop"), None);
        assert_eq!(id("a.txt"), None);
        assert_eq!(id(".desktop"), None);
        assert_eq!(
            DesktopId::new("org.a.b.desktop").unwrap().app_id(),
            "org.a.b"
        );
    }

    #[test]
    fn parses_an_application() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=Editor\nName[de]=Bearbeiter\n\
             GenericName=Text Editor\nComment=Edit\nIcon=accessories-text-editor\n\
             Exec=edit %F\nTryExec=edit\nPath=/tmp\nTerminal=false\nNoDisplay=true\n\
             OnlyShowIn=KDE;GNOME;\nCategories=Utility;TextEditor;\nKeywords=text;notes;\n\
             Keywords[de]=Text;\nMimeType=text/plain;\nStartupWMClass=editor\n\
             StartupNotify=true\nActions=new;missing;\n\
             [Desktop Action new]\nName=New Window\nExec=edit --new\n\
             [Desktop Action unlisted]\nName=Unlisted\nExec=x\n",
        )
        .unwrap();
        assert_eq!(
            entry.name.get(&[Locale::parse("de").unwrap()]),
            "Bearbeiter"
        );
        assert_eq!(entry.generic_name.unwrap().default, "Text Editor");
        assert_eq!(entry.icon.unwrap().default, "accessories-text-editor");
        assert!(entry.no_display && !entry.hidden);
        assert_eq!(entry.only_show_in, ["KDE", "GNOME"]);
        assert_eq!(entry.categories, ["Utility", "TextEditor"]);
        let keywords = entry.keywords.unwrap();
        assert_eq!(keywords.default, ["text", "notes"]);
        assert_eq!(keywords.get(&[Locale::parse("de").unwrap()]), &["Text"]);
        let EntryKind::Application(app) = entry.kind else {
            panic!("not an application");
        };
        assert_eq!(app.exec.as_deref(), Some("edit %F"));
        assert_eq!(app.try_exec.as_deref(), Some("edit"));
        assert_eq!(app.working_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(app.mime_types, ["text/plain"]);
        assert_eq!(app.startup_wm_class.as_deref(), Some("editor"));
        assert!(app.startup_notify && !app.terminal && !app.dbus_activatable);
        assert_eq!(app.actions.len(), 1);
        assert_eq!(app.actions[0].id, "new");
        assert_eq!(app.actions[0].name.default, "New Window");
        assert_eq!(app.actions[0].exec.as_deref(), Some("edit --new"));
    }

    #[test]
    fn parses_links_and_directories() {
        let link = parse("[Desktop Entry]\nType=Link\nName=Site\nURL=https://example.org\n");
        assert_eq!(
            link.unwrap().kind,
            EntryKind::Link {
                url: "https://example.org".into()
            }
        );
        let dir = parse("[Desktop Entry]\nType=Directory\nName=Games\n").unwrap();
        assert_eq!(dir.kind, EntryKind::Directory);
        assert!(dir.application().is_none());
    }

    #[test]
    fn a_dbus_activatable_application_needs_no_exec() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=A\nDBusActivatable=true\nActions=a;\n\
             [Desktop Action a]\nName=Act\n",
        )
        .unwrap();
        let app = entry.application().unwrap();
        assert!(app.dbus_activatable && app.exec.is_none() && app.actions[0].exec.is_none());
    }

    #[test]
    fn rejects_entries_missing_what_their_type_requires() {
        for (text, key) in [
            ("[Desktop Entry]\nName=A\n", "Type"),
            ("[Desktop Entry]\nType=Application\nExec=a\n", "Name"),
            ("[Desktop Entry]\nType=Application\nName=A\n", "Exec"),
            ("[Desktop Entry]\nType=Link\nName=A\n", "URL"),
            (
                "[Desktop Entry]\nType=Application\nName=A\nExec=a\nActions=x;\n\
                 [Desktop Action x]\nName=X\n",
                "Exec",
            ),
        ] {
            assert_eq!(parse(text), Err(ParseError::MissingKey { key }), "{text:?}");
        }
        assert_eq!(
            parse("[Other]\nType=Application\n"),
            Err(ParseError::MissingDesktopEntryGroup)
        );
        assert_eq!(
            parse("[Desktop Entry]\nType=Service\nName=A\n"),
            Err(ParseError::UnknownType {
                value: "Service".into()
            })
        );
    }
}
