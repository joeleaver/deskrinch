//! [`Applications`]: the installed entries, every one by its ID, kept up to
//! date from the catalog's changes.

use std::collections::BTreeMap;
use std::sync::Arc;

use deskrinch_desktop_entries::{
    Catalog, CatalogEntry, DesktopEntry, DesktopId, Event, Visibility, entry_for_app_id,
};

/// The catalog as a component keeps it: every installed entry by its ID,
/// shown in menus or not, changed by the watcher's [`Event`]s. The entries
/// are the catalog's own, shared: cloning this clones a map, never an entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applications {
    entries: BTreeMap<DesktopId, Arc<CatalogEntry>>,
}

impl Applications {
    /// The entries of `catalog`.
    #[must_use]
    pub fn new(catalog: &Catalog) -> Self {
        Self {
            entries: catalog
                .iter()
                .map(|entry| (entry.entry.id.clone(), Arc::clone(entry)))
                .collect(),
        }
    }

    /// Applies one change; answers whether anything changed. An
    /// [`Event::Error`] changes nothing.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Added(entry) | Event::Changed(entry) => {
                let id = entry.entry.id.clone();
                self.entries.insert(id, Arc::clone(&entry)) != Some(entry)
            }
            Event::Removed(id) => self.entries.remove(&id).is_some(),
            Event::Error(_) => false,
        }
    }

    /// The applications shown in menus, by ID: the entries themselves,
    /// shared.
    #[must_use]
    pub fn shown(&self) -> Arc<Vec<Arc<CatalogEntry>>> {
        Arc::new(
            self.entries
                .values()
                .filter(|entry| entry.visibility == Visibility::Shown)
                .map(Arc::clone)
                .collect(),
        )
    }

    /// The entry with this ID, shown or not.
    #[must_use]
    pub fn get(&self, id: &DesktopId) -> Option<&DesktopEntry> {
        self.entries.get(id).map(|entry| &entry.entry)
    }

    /// The application the windows with `app_id` belong to, by
    /// [`entry_for_app_id`]'s rules, among every entry, shown or not.
    #[must_use]
    pub fn for_app_id(&self, app_id: &str) -> Option<&DesktopEntry> {
        entry_for_app_id(self.entries.values().map(|entry| &entry.entry), app_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, visibility: Visibility) -> Arc<CatalogEntry> {
        let text = format!("[Desktop Entry]\nType=Application\nName={id}\nExec=true\n");
        let id = DesktopId::new(format!("{id}.desktop")).unwrap();
        Arc::new(CatalogEntry {
            entry: DesktopEntry::parse(id, "/x".into(), &text, &[]).unwrap(),
            visibility,
        })
    }

    fn id(name: &str) -> DesktopId {
        DesktopId::new(format!("{name}.desktop")).unwrap()
    }

    fn shown(applications: &Applications) -> Vec<String> {
        applications
            .shown()
            .iter()
            .map(|entry| entry.entry.id.app_id().to_owned())
            .collect()
    }

    #[test]
    fn follows_installs_changes_and_removals() {
        let mut applications = Applications::default();
        assert!(applications.apply(Event::Added(entry("b", Visibility::Shown))));
        assert!(applications.apply(Event::Added(entry("a", Visibility::Shown))));
        assert_eq!(shown(&applications), ["a", "b"]);
        // Hidden by NoDisplay: it leaves the shown list, and stays installed.
        assert!(applications.apply(Event::Changed(entry("a", Visibility::NoDisplay))));
        assert_eq!(shown(&applications), ["b"]);
        assert!(applications.get(&id("a")).is_some());
        assert!(applications.apply(Event::Removed(id("b"))));
        assert_eq!(shown(&applications), Vec::<String>::new());
    }

    #[test]
    fn the_catalog_the_state_and_a_delivery_share_each_entry() {
        let data = tempfile::tempdir().unwrap();
        let applications_dir = data.path().join("applications");
        std::fs::create_dir_all(&applications_dir).unwrap();
        std::fs::write(
            applications_dir.join("a.desktop"),
            "[Desktop Entry]\nType=Application\nName=A\nExec=true\n",
        )
        .unwrap();
        let env = deskrinch_desktop_entries::Environment {
            base_dirs: deskrinch_desktop_entries::BaseDirs {
                data_home: Some(data.path().to_path_buf()),
                ..deskrinch_desktop_entries::BaseDirs::default()
            },
            path: vec!["/usr/bin".into(), "/bin".into()],
            ..deskrinch_desktop_entries::Environment::default()
        };
        let catalog = Catalog::scan(&env);
        let in_catalog = catalog.get(&id("a")).expect("scanned");
        let state = Applications::new(&catalog);
        let delivered = state.clone();
        assert!(Arc::ptr_eq(in_catalog, &state.entries[&id("a")]));
        assert!(Arc::ptr_eq(in_catalog, &delivered.entries[&id("a")]));
        assert!(Arc::ptr_eq(in_catalog, &delivered.shown()[0]));
    }

    #[test]
    fn an_event_s_entry_is_the_one_kept() {
        let mut applications = Applications::default();
        let added = entry("a", Visibility::Shown);
        applications.apply(Event::Added(Arc::clone(&added)));
        assert!(Arc::ptr_eq(&added, &applications.clone().entries[&id("a")]));
    }

    #[test]
    fn what_does_not_change_anything_says_so() {
        let mut applications = Applications::default();
        assert!(applications.apply(Event::Added(entry("x", Visibility::NoDisplay))));
        assert!(
            !applications.apply(Event::Changed(entry("x", Visibility::NoDisplay))),
            "the same entry again"
        );
        assert!(applications.apply(Event::Removed(id("x"))));
        assert!(!applications.apply(Event::Removed(id("never"))));
    }

    #[test]
    fn an_app_id_finds_an_entry_not_shown_in_menus() {
        let mut applications = Applications::default();
        applications.apply(Event::Added(entry(
            "org.example.Helper",
            Visibility::NoDisplay,
        )));
        let found = applications.for_app_id("org.example.Helper");
        assert_eq!(
            found.map(|entry| entry.id.app_id()),
            Some("org.example.Helper")
        );
        assert!(applications.for_app_id("org.example.Other").is_none());
    }
}
