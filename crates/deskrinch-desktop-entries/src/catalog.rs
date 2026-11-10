//! The set of installed entries: precedence by desktop-file ID, and whether
//! each is shown. This module is pure; [`crate::scan`] feeds it from disk.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry as MapEntry;

use crate::entry::{DesktopEntry, DesktopId};
use crate::env::Environment;
use crate::error::Problem;

/// Whether an entry belongs in menus and launchers, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Shown.
    Shown,
    /// `NoDisplay=true`.
    NoDisplay,
    /// `OnlyShowIn` leaves out, or `NotShowIn` names, the current desktop.
    NotInCurrentDesktop,
    /// The `TryExec` program is not installed.
    TryExecMissing,
}

/// An entry in the catalog, with its visibility in the environment it was
/// scanned in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// The entry.
    pub entry: DesktopEntry,
    /// Whether it is shown.
    pub visibility: Visibility,
}

/// The installed desktop entries, one per desktop-file ID.
///
/// Built by the Desktop Entry Specification's rules:
///
/// - The data directories are searched in order (`$XDG_DATA_HOME`, then
///   `$XDG_DATA_DIRS`), each in its `applications` subdirectory, and the
///   first file with a given ID wins.
/// - A winning file with `Hidden=true` deletes the ID: it hides the entries
///   further down as well as itself.
/// - A file that cannot be read or is invalid is ignored, so a valid entry
///   further down takes the ID. It is listed in [`Catalog::problems`].
/// - Every other entry is kept, with a [`Visibility`]: `NoDisplay`, then
///   `OnlyShowIn`/`NotShowIn` against `$XDG_CURRENT_DESKTOP`, then
///   `TryExec`. A launcher shows only [`Catalog::shown`]; a taskbar matching
///   windows to applications uses every entry.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: BTreeMap<DesktopId, CatalogEntry>,
    problems: Vec<Problem>,
}

/// One file found during a scan: the entry, or why it could not be read or
/// parsed.
pub(crate) type Found = Result<DesktopEntry, Problem>;

impl Catalog {
    /// Builds the catalog from `found`, every file in precedence order
    /// (best first). `try_exec_exists` says whether a `TryExec` program
    /// exists; only entries that pass the other checks are probed.
    pub(crate) fn build(
        found: impl IntoIterator<Item = Found>,
        env: &Environment,
        try_exec_exists: impl Fn(&str) -> bool,
    ) -> Self {
        // `None` marks an ID deleted by `Hidden`.
        let mut winners: BTreeMap<DesktopId, Option<DesktopEntry>> = BTreeMap::new();
        let mut problems = Vec::new();
        for found in found {
            match found {
                Err(problem) => problems.push(problem),
                Ok(entry) => {
                    if let MapEntry::Vacant(slot) = winners.entry(entry.id.clone()) {
                        slot.insert((!entry.hidden).then_some(entry));
                    }
                }
            }
        }
        let entries = winners
            .into_iter()
            .filter_map(|(id, entry)| {
                let entry = entry?;
                let visibility = visibility(&entry, &env.current_desktops, &try_exec_exists);
                Some((id, CatalogEntry { entry, visibility }))
            })
            .collect();
        Self { entries, problems }
    }

    /// The entry with this ID.
    #[must_use]
    pub fn get(&self, id: &DesktopId) -> Option<&CatalogEntry> {
        self.entries.get(id)
    }

    /// Every entry, shown or not, in ID order.
    pub fn iter(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.values()
    }

    /// The entries a launcher shows, in ID order.
    pub fn shown(&self) -> impl Iterator<Item = &DesktopEntry> {
        self.entries
            .values()
            .filter(|e| e.visibility == Visibility::Shown)
            .map(|e| &e.entry)
    }

    /// How many entries there are, shown or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The files the scan skipped, and why.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub(crate) fn entries(&self) -> &BTreeMap<DesktopId, CatalogEntry> {
        &self.entries
    }
}

fn visibility(
    entry: &DesktopEntry,
    current_desktops: &[String],
    try_exec_exists: impl Fn(&str) -> bool,
) -> Visibility {
    if entry.no_display {
        return Visibility::NoDisplay;
    }
    let is_current = |names: &[String]| names.iter().any(|name| current_desktops.contains(name));
    if (!entry.only_show_in.is_empty() && !is_current(&entry.only_show_in))
        || is_current(&entry.not_show_in)
    {
        return Visibility::NotInCurrentDesktop;
    }
    let try_exec = entry.application().and_then(|app| app.try_exec.as_deref());
    if try_exec.is_some_and(|program| !try_exec_exists(program)) {
        return Visibility::TryExecMissing;
    }
    Visibility::Shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ParseError, ScanError};

    fn entry(id: &str, origin: &str, extra: &str) -> DesktopEntry {
        let text = format!("[Desktop Entry]\nType=Application\nName={origin}\nExec=x\n{extra}");
        let id = DesktopId::new(id).unwrap();
        let path = format!("/{origin}/{id}").into();
        DesktopEntry::parse(id, path, &text).unwrap()
    }

    fn env(desktops: &[&str]) -> Environment {
        Environment {
            current_desktops: desktops.iter().map(|d| (*d).to_owned()).collect(),
            ..Environment::default()
        }
    }

    fn names(catalog: &Catalog) -> Vec<(String, String)> {
        catalog
            .iter()
            .map(|e| (e.entry.id.to_string(), e.entry.name.default.clone()))
            .collect()
    }

    #[test]
    fn the_first_entry_of_an_id_wins() {
        let catalog = Catalog::build(
            [
                entry("a.desktop", "home", ""),
                entry("b.desktop", "home", ""),
                entry("a.desktop", "system", ""),
                entry("c.desktop", "system", ""),
            ]
            .map(Ok),
            &env(&[]),
            |_| true,
        );
        assert_eq!(
            names(&catalog),
            [
                ("a.desktop".into(), "home".into()),
                ("b.desktop".into(), "home".into()),
                ("c.desktop".into(), "system".into()),
            ]
        );
    }

    #[test]
    fn hidden_deletes_the_id_further_down() {
        let catalog = Catalog::build(
            [
                entry("a.desktop", "home", "Hidden=true\n"),
                entry("a.desktop", "system", ""),
                entry("b.desktop", "system", "Hidden=true\n"),
            ]
            .map(Ok),
            &env(&[]),
            |_| true,
        );
        assert!(catalog.is_empty());
    }

    #[test]
    fn an_invalid_file_falls_through_to_the_next() {
        let problem = Problem {
            path: "/home/a.desktop".into(),
            error: ScanError::Parse(ParseError::MissingDesktopEntryGroup),
        };
        let catalog = Catalog::build(
            [Err(problem), Ok(entry("a.desktop", "system", ""))],
            &env(&[]),
            |_| true,
        );
        assert_eq!(names(&catalog), [("a.desktop".into(), "system".into())]);
        assert_eq!(catalog.problems().len(), 1);
    }

    #[test]
    fn decides_visibility() {
        let catalog = Catalog::build(
            [
                entry(
                    "shown.desktop",
                    "s",
                    "OnlyShowIn=GNOME;KDE;\nTryExec=present\n",
                ),
                entry("nodisplay.desktop", "s", "NoDisplay=true\n"),
                entry("only.desktop", "s", "OnlyShowIn=GNOME;\n"),
                entry("not.desktop", "s", "NotShowIn=KDE;\n"),
                entry("tryexec.desktop", "s", "TryExec=absent\n"),
            ]
            .map(Ok),
            &env(&["deskrinch", "KDE"]),
            |program| program == "present",
        );
        let visibility = |id: &str| {
            catalog
                .get(&DesktopId::new(id).unwrap())
                .unwrap()
                .visibility
        };
        assert_eq!(visibility("shown.desktop"), Visibility::Shown);
        assert_eq!(visibility("nodisplay.desktop"), Visibility::NoDisplay);
        assert_eq!(visibility("only.desktop"), Visibility::NotInCurrentDesktop);
        assert_eq!(visibility("not.desktop"), Visibility::NotInCurrentDesktop);
        assert_eq!(visibility("tryexec.desktop"), Visibility::TryExecMissing);
        let shown: Vec<_> = catalog.shown().map(|e| e.id.to_string()).collect();
        assert_eq!(shown, ["shown.desktop"]);
    }

    #[test]
    fn only_show_in_hides_when_no_desktop_is_known() {
        let catalog = Catalog::build(
            [entry("a.desktop", "s", "OnlyShowIn=KDE;\n")].map(Ok),
            &env(&[]),
            |_| true,
        );
        assert_eq!(catalog.shown().count(), 0);
    }
}
