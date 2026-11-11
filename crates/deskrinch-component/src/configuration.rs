//! The process's configuration: its files, and the one watcher on them.

use std::fmt;
use std::sync::Arc;

use deskrinch_config::{Section, Settings, Sources, Watcher, load};
use deskrinch_settings::{Table, section};
use deskrinch_xdg::BaseDirs;

use crate::live::LiveSettings;
use crate::report;

/// The configuration of a component's process: the files, and the one
/// [`Watcher`] that follows them, which every table the component reads
/// shares.
///
/// [`start`](Self::start) it once; [`follow`](Self::follow) each table.
/// Every [`LiveSettings`] holds the watcher, so it runs for as long as any
/// table follows it, whether or not this value is kept.
#[derive(Clone)]
pub struct Configuration {
    shared: Arc<Shared>,
}

/// What the tables share.
pub(crate) struct Shared {
    pub(crate) sources: Sources,
    /// `None` when the files cannot be watched: each table is then read
    /// once.
    pub(crate) watcher: Option<Watcher>,
}

impl Configuration {
    /// The configuration in `dirs`, watched. When the files cannot be
    /// watched (which is reported), every table is read once and does not
    /// follow changes.
    #[must_use]
    pub fn start(dirs: &BaseDirs) -> Self {
        let sources = Sources::new(dirs);
        let watcher = match Watcher::start(sources.clone(), |error| {
            report::error("watching the configuration", &error);
        }) {
            Ok(watcher) => Some(watcher),
            Err(error) => {
                report::error("watching the configuration (it is read once)", &error);
                None
            }
        };
        Self {
            shared: Arc::new(Shared { sources, watcher }),
        }
    }

    /// Table `T`, read now and followed while the watcher runs, with the
    /// [known tables](deskrinch_settings::KNOWN_TABLES) applied.
    #[must_use]
    pub fn follow<T: Table>(&self) -> LiveSettings<T> {
        match section::<T>() {
            Ok(section) => self.follow_section(section),
            Err(error) => {
                report::error("the settings' defaults", &error);
                LiveSettings::fixed(T::default(), Arc::clone(&self.shared))
            }
        }
    }

    /// `section`, read now and followed while the watcher runs.
    pub(crate) fn follow_section<T: Settings>(&self, section: Section<T>) -> LiveSettings<T> {
        if let Some(watcher) = &self.shared.watcher {
            return LiveSettings::subscribe(watcher, section, Arc::clone(&self.shared));
        }
        let resolved = load(&self.shared.sources, section);
        resolved.diagnostics.iter().for_each(report::diagnostic);
        LiveSettings::fixed(resolved.settings, Arc::clone(&self.shared))
    }

    /// Whether changes to the files are followed.
    #[must_use]
    pub fn is_watched(&self) -> bool {
        self.shared.watcher.is_some()
    }
}

impl fmt::Debug for Configuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Configuration")
            .field("files", &self.shared.sources.files().collect::<Vec<_>>())
            .field("watched", &self.is_watched())
            .finish()
    }
}
