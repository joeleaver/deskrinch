//! The set of installed entries: precedence by desktop-file ID, and whether
//! each is shown. This module is pure; [`crate::scan`] feeds it from disk.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry as MapEntry;
use std::path::Path;
use std::sync::Arc;

use crate::entry::{DesktopEntry, DesktopId};
use crate::env::Environment;
use crate::error::{Problem, ProblemKind};
use crate::exec;

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
    /// The application cannot be started: the program its `Exec` runs is
    /// not installed, or it has no usable `Exec` and is not
    /// `DBusActivatable`. Typically a desktop file an uninstalled
    /// application left behind. GNOME hides these too.
    ExecMissing,
}

/// An entry in the catalog, with its visibility in the environment it was
/// scanned in. Immutable once built, and shared (`Arc`) by everything that
/// holds the catalog: a change replaces an entry, never edits it.
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
///   `TryExec`, then whether the application can be started. A launcher
///   shows only [`Catalog::shown`]; a taskbar matching windows to
///   applications uses every entry.
///
/// Whether an application can be started is judged by what would launch it:
///
/// - A `DBusActivatable` entry is launched by D-Bus activation, so its
///   `Exec` does not decide: it is shown whether or not it has one and
///   whether or not that program exists. (Whether the bus can activate it
///   is not checked; that would mean reading the bus's service files.)
/// - Any other application runs the program its `Exec` names, resolved as
///   the launch resolves it: an absolute path as it is; a path containing
///   `/` relative to the entry's `Path` (or the current directory); a bare
///   name by searching the environment's `PATH`. It must be an executable
///   file. An entry without `Exec`, or whose `Exec` is invalid or starts
///   with a field code, cannot be started either. Each of these is
///   [`Visibility::ExecMissing`].
///
/// `TryExec` and `Exec` programs are checked when the catalog is built.
/// `PATH` is not watched, so a program installed or removed later leaves
/// the visibility stale until the next scan, for `Exec` exactly as for
/// `TryExec`; [`crate::Watcher::refresh`] rescans on demand.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: BTreeMap<DesktopId, Arc<CatalogEntry>>,
    problems: Vec<Problem>,
}

/// A program whose presence decides whether an entry is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Program<'a> {
    /// A `TryExec` value: an absolute path, or looked up in `PATH`, as the
    /// specification says.
    TryExec(&'a str),
    /// The program an `Exec` runs, resolved as a launch resolves it.
    Exec {
        /// The program as written.
        program: &'a str,
        /// The entry's `Path`, which a relative path is resolved against.
        working_dir: Option<&'a Path>,
    },
}

/// One file found during a scan: the entry, or why it could not be read or
/// parsed.
pub(crate) type Found = Result<DesktopEntry, Problem>;

impl Catalog {
    /// Builds the catalog from `found`, every file in precedence order
    /// (best first). `program_exists` says whether a [`Program`] is an
    /// installed executable; only entries that pass the other checks are
    /// probed.
    pub(crate) fn build(
        found: impl IntoIterator<Item = Found>,
        env: &Environment,
        program_exists: impl Fn(Program<'_>) -> bool,
    ) -> Self {
        // `None` marks an ID deleted by `Hidden`.
        let mut winners: BTreeMap<DesktopId, Option<DesktopEntry>> = BTreeMap::new();
        let mut problems = Vec::new();
        for found in found {
            match found {
                Err(problem) => problems.push(problem),
                Ok(entry) => {
                    problems.extend(entry.deviations.iter().map(|deviation| Problem {
                        path: entry.path.clone(),
                        kind: ProblemKind::Deviation(deviation.clone()),
                    }));
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
                let visibility = visibility(&entry, &env.current_desktops, &program_exists);
                Some((id, Arc::new(CatalogEntry { entry, visibility })))
            })
            .collect();
        Self { entries, problems }
    }

    /// The entry with this ID.
    #[must_use]
    pub fn get(&self, id: &DesktopId) -> Option<&Arc<CatalogEntry>> {
        self.entries.get(id)
    }

    /// Every entry, shown or not, in ID order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<CatalogEntry>> {
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

    /// What was wrong with the files the scan read, in the order read: the
    /// files it skipped, and the deviations from the specification in the
    /// files it read anyway (see [`Problem::is_skipped`]).
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub(crate) fn entries(&self) -> &BTreeMap<DesktopId, Arc<CatalogEntry>> {
        &self.entries
    }

    /// This catalog, holding `older`'s `Arc` in place of every entry equal
    /// to the one `older` has for its ID: after a rescan, an entry that did
    /// not change stays the one allocation everything already shares.
    #[must_use]
    pub(crate) fn reusing(mut self, older: &Catalog) -> Self {
        for (id, entry) in &mut self.entries {
            if let Some(old) = older.entries.get(id)
                && old == entry
            {
                *entry = Arc::clone(old);
            }
        }
        self
    }
}

fn visibility(
    entry: &DesktopEntry,
    current_desktops: &[String],
    program_exists: impl Fn(Program<'_>) -> bool,
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
    let Some(app) = entry.application() else {
        return Visibility::Shown;
    };
    if app
        .try_exec
        .as_deref()
        .is_some_and(|program| !program_exists(Program::TryExec(program)))
    {
        return Visibility::TryExecMissing;
    }
    if app.dbus_activatable {
        return Visibility::Shown;
    }
    // A command line `launch` would refuse cannot start either.
    let fields = exec::Fields {
        name: "",
        icon: None,
        desktop_file: None,
    };
    let startable = app
        .exec
        .as_deref()
        .filter(|command| exec::expand(command, &fields, &[]).is_ok())
        .and_then(exec::program)
        .is_some_and(|program| {
            program_exists(Program::Exec {
                program: &program,
                working_dir: app.working_dir.as_deref(),
            })
        });
    if startable {
        Visibility::Shown
    } else {
        Visibility::ExecMissing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ParseError, ScanError};

    fn entry(id: &str, origin: &str, extra: &str) -> DesktopEntry {
        let text = format!("[Desktop Entry]\nType=Application\nName={origin}\nExec=x\n{extra}");
        let id = DesktopId::new(id).unwrap();
        let path = format!("/{origin}/{id}").into();
        DesktopEntry::parse(id, path, &text, &[]).unwrap()
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
            .map(|e| (e.entry.id.to_string(), e.entry.name.text.clone()))
            .collect()
    }

    fn built(entries: impl IntoIterator<Item = DesktopEntry>) -> Catalog {
        Catalog::build(entries.into_iter().map(Ok), &env(&[]), |_| true)
    }

    #[test]
    fn a_rebuild_keeps_the_unchanged_entries_shared_and_replaces_the_changed_one() {
        let older = built([
            entry("a.desktop", "home", ""),
            entry("b.desktop", "home", ""),
            entry("c.desktop", "home", ""),
        ]);
        let newer = built([
            entry("a.desktop", "home", ""),
            entry("b.desktop", "home", "Comment=changed\n"),
            entry("c.desktop", "home", ""),
        ])
        .reusing(&older);
        let shared = |id: &str| {
            let id = DesktopId::new(id).unwrap();
            Arc::ptr_eq(older.get(&id).unwrap(), newer.get(&id).unwrap())
        };
        assert!(shared("a.desktop") && shared("c.desktop"));
        assert!(!shared("b.desktop"));
        let events = crate::watch::diff(&older, &newer);
        assert_eq!(events.len(), 1, "{events:?}");
        let crate::watch::Event::Changed(changed) = &events[0] else {
            panic!("{events:?}");
        };
        assert!(Arc::ptr_eq(
            changed,
            newer.get(&DesktopId::new("b.desktop").unwrap()).unwrap()
        ));
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
            kind: ProblemKind::Skipped(ScanError::Parse(ParseError::MissingDesktopEntryGroup)),
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
            |program| program != Program::TryExec("absent"),
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

    /// An application entry whose `[Desktop Entry]` body is `body`.
    fn raw(id: &str, body: &str) -> DesktopEntry {
        let text = format!("[Desktop Entry]\nType=Application\nName=N\n{body}");
        let path = format!("/s/{id}").into();
        DesktopEntry::parse(DesktopId::new(id).unwrap(), path, &text, &[]).unwrap()
    }

    #[test]
    fn an_application_that_cannot_start_is_not_shown() {
        let probed = std::cell::RefCell::new(Vec::new());
        let catalog = Catalog::build(
            [
                raw("installed.desktop", "Exec=installed --flag %U\n"),
                raw("gone.desktop", "Exec=\"/opt/gone/bin/app\" %F\n"),
                raw("no-exec.desktop", ""),
                raw("field-code.desktop", "Exec=%f\n"),
                raw("unterminated.desktop", "Exec=installed \"open\n"),
                raw("unknown-code.desktop", "Exec=installed %z\n"),
                raw("relative.desktop", "Exec=./run\nPath=/opt/app\n"),
                // D-Bus activation launches these, not `Exec`.
                raw("dbus.desktop", "DBusActivatable=true\n"),
                raw("dbus-gone.desktop", "DBusActivatable=true\nExec=gone\n"),
                // `TryExec` is reported first.
                raw("both.desktop", "TryExec=gone\nExec=gone\n"),
            ]
            .map(Ok),
            &env(&[]),
            |program| match program {
                Program::TryExec(_) => false,
                Program::Exec {
                    program,
                    working_dir,
                } => {
                    probed
                        .borrow_mut()
                        .push((program.to_owned(), working_dir.map(Path::to_path_buf)));
                    program == "installed" || program == "./run"
                }
            },
        );
        let visibility = |id: &str| {
            catalog
                .get(&DesktopId::new(id).unwrap())
                .unwrap()
                .visibility
        };
        for (id, expected) in [
            ("installed.desktop", Visibility::Shown),
            ("gone.desktop", Visibility::ExecMissing),
            ("no-exec.desktop", Visibility::ExecMissing),
            ("field-code.desktop", Visibility::ExecMissing),
            ("unterminated.desktop", Visibility::ExecMissing),
            ("unknown-code.desktop", Visibility::ExecMissing),
            ("relative.desktop", Visibility::Shown),
            ("dbus.desktop", Visibility::Shown),
            ("dbus-gone.desktop", Visibility::Shown),
            ("both.desktop", Visibility::TryExecMissing),
        ] {
            assert_eq!(visibility(id), expected, "{id}");
        }
        // The program is probed as written, with the entry's `Path`.
        assert!(
            probed
                .borrow()
                .contains(&("./run".into(), Some("/opt/app".into())))
        );
        assert!(
            probed
                .borrow()
                .contains(&("/opt/gone/bin/app".into(), None))
        );
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
