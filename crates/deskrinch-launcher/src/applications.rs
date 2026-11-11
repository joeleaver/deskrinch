//! The applications the launcher offers: the installed entries shown in
//! menus, kept up to date from the catalog's changes.

use std::collections::BTreeMap;
use std::sync::Arc;

use deskrinch_desktop_entries::{
    Catalog, CatalogEntry, DesktopEntry, DesktopId, Event, Visibility,
};

/// The catalog as the launcher keeps it: every entry by its ID, changed by
/// the watcher's [`Event`]s.
#[derive(Debug, Clone, Default)]
pub struct Applications {
    entries: BTreeMap<DesktopId, CatalogEntry>,
}

impl Applications {
    /// The entries of `catalog`.
    #[must_use]
    pub fn new(catalog: &Catalog) -> Self {
        Self {
            entries: catalog
                .iter()
                .map(|entry| (entry.entry.id.clone(), entry.clone()))
                .collect(),
        }
    }

    /// Applies one change; answers whether the applications shown changed.
    /// An [`Event::Error`] changes nothing.
    pub fn apply(&mut self, event: Event) -> bool {
        match event {
            Event::Added(entry) | Event::Changed(entry) => {
                let shown = entry.visibility == Visibility::Shown;
                let id = entry.entry.id.clone();
                let was = self.entries.insert(id, entry);
                shown || was.is_some_and(|was| was.visibility == Visibility::Shown)
            }
            Event::Removed(id) => self
                .entries
                .remove(&id)
                .is_some_and(|was| was.visibility == Visibility::Shown),
            Event::Error(_) => false,
        }
    }

    /// The applications shown in menus, by ID.
    #[must_use]
    pub fn shown(&self) -> Arc<Vec<DesktopEntry>> {
        Arc::new(
            self.entries
                .values()
                .filter(|entry| entry.visibility == Visibility::Shown)
                .map(|entry| entry.entry.clone())
                .collect(),
        )
    }

    /// The entry with this ID, shown or not.
    #[must_use]
    pub fn get(&self, id: &DesktopId) -> Option<&DesktopEntry> {
        self.entries.get(id).map(|entry| &entry.entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, visibility: Visibility) -> CatalogEntry {
        let text = format!("[Desktop Entry]\nType=Application\nName={id}\nExec=true\n");
        let id = DesktopId::new(format!("{id}.desktop")).unwrap();
        CatalogEntry {
            entry: DesktopEntry::parse(id, "/x".into(), &text).unwrap(),
            visibility,
        }
    }

    fn ids(applications: &Applications) -> Vec<String> {
        applications
            .shown()
            .iter()
            .map(|entry| entry.id.app_id().to_owned())
            .collect()
    }

    #[test]
    fn follows_installs_changes_and_removals_of_shown_entries() {
        let mut applications = Applications::default();
        assert!(applications.apply(Event::Added(entry("b", Visibility::Shown))));
        assert!(applications.apply(Event::Added(entry("a", Visibility::Shown))));
        assert_eq!(ids(&applications), ["a", "b"]);
        // Hidden by NoDisplay: it leaves the list.
        assert!(applications.apply(Event::Changed(entry("a", Visibility::NoDisplay))));
        assert_eq!(ids(&applications), ["b"]);
        assert!(
            applications
                .get(&DesktopId::new("a.desktop").unwrap())
                .is_some()
        );
        assert!(applications.apply(Event::Removed(DesktopId::new("b.desktop").unwrap())));
        assert_eq!(ids(&applications), Vec::<String>::new());
    }

    #[test]
    fn a_change_to_what_is_not_shown_changes_nothing_shown() {
        let mut applications = Applications::default();
        assert!(!applications.apply(Event::Added(entry("x", Visibility::NoDisplay))));
        assert!(!applications.apply(Event::Removed(DesktopId::new("x.desktop").unwrap())));
        assert!(!applications.apply(Event::Removed(DesktopId::new("never.desktop").unwrap())));
    }
}
