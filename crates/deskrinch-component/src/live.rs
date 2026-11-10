//! A component's settings while it runs.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_config::{Event, Sources, Subscription, Watcher, load};
use deskrinch_settings::{Table, section};
use deskrinch_xdg::BaseDirs;
use rinch::prelude::Signal;

use crate::report;

/// A component's table `T` of the configuration: read when the component
/// starts, and followed while it runs. Kept for as long as the component
/// runs; dropping it stops following.
///
/// [`start`](Self::start) starts the process's one configuration
/// [`Watcher`] and subscribes the table, with the known tables applied
/// ([`deskrinch_settings::section`]). The settings that returns are
/// [`initial`](Self::initial), which the component makes its window with.
/// Every later change is held until the UI's signal exists
/// ([`connect`](Self::connect)), then sent to it with
/// [`Signal::send`], rinch's write from another thread. Diagnostics are
/// [reported](report::diagnostic) as they come; a broken file leaves the
/// last good settings in place (the configuration crate's rule).
pub struct LiveSettings<T: Table> {
    /// The settings when the component started.
    pub initial: T,
    inbox: Arc<Inbox<T>>,
    // The subscription before the watcher: deliveries stop, then the
    // watcher's thread.
    _subscription: Option<Subscription>,
    watcher: Option<Watcher>,
}

impl<T: Table> LiveSettings<T> {
    /// Reads table `T` from the configuration in `dirs` and follows it.
    /// When the files cannot be watched (which is reported), the table is
    /// read once and does not follow changes.
    pub fn start(dirs: &BaseDirs) -> Self {
        let sources = Sources::new(dirs);
        let section = match section::<T>() {
            Ok(section) => section,
            Err(error) => {
                report::error("the settings' defaults", &error);
                return Self::fixed(T::default());
            }
        };
        let watcher = match Watcher::start(sources.clone(), |error| {
            report::error("watching the configuration", &error);
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                report::error("watching the configuration (it is read once)", &error);
                let resolved = load(&sources, section);
                resolved.diagnostics.iter().for_each(report::diagnostic);
                return Self::fixed(resolved.settings);
            }
        };
        let inbox = Arc::new(Inbox::default());
        let deliver = Arc::clone(&inbox);
        let (subscription, resolved) = watcher.subscribe(section, move |event| match event {
            Event::Changed(settings) => deliver.deliver(settings),
            Event::Diagnostics(diagnostics) => diagnostics.iter().for_each(report::diagnostic),
        });
        resolved.diagnostics.iter().for_each(report::diagnostic);
        Self {
            initial: resolved.settings,
            inbox,
            _subscription: Some(subscription),
            watcher: Some(watcher),
        }
    }

    fn fixed(initial: T) -> Self {
        Self {
            initial,
            inbox: Arc::default(),
            _subscription: None,
            watcher: None,
        }
    }

    /// Sends every later change to `signal`, and returns the newest change
    /// that came before, if any. Call it on the UI thread, once.
    pub fn connect(&self, signal: Signal<T>) -> Option<T> {
        self.inbox.connect(signal)
    }
}

impl<T: Table + fmt::Debug> fmt::Debug for LiveSettings<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveSettings")
            .field("table", &T::NAME)
            .field("initial", &self.initial)
            .field("following", &self.watcher.is_some())
            .finish_non_exhaustive()
    }
}

/// Where the watcher's thread hands the settings to the UI.
struct Inbox<T: 'static>(Mutex<Target<T>>);

enum Target<T: 'static> {
    /// No signal yet; the newest change so far.
    Waiting(Option<T>),
    /// The UI's signal.
    Connected(Signal<T>),
}

impl<T> Default for Inbox<T> {
    fn default() -> Self {
        Self(Mutex::new(Target::Waiting(None)))
    }
}

impl<T: Send> Inbox<T> {
    fn deliver(&self, settings: T) {
        // A panic while holding the lock leaves a valid `Target`.
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match &mut *target {
            Target::Waiting(pending) => *pending = Some(settings),
            Target::Connected(signal) => signal.send(settings),
        }
    }

    fn connect(&self, signal: Signal<T>) -> Option<T> {
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *target, Target::Connected(signal)) {
            Target::Waiting(pending) => pending,
            Target::Connected(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_settings::panel::{Edge, PanelSettings};
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn top() -> PanelSettings {
        PanelSettings {
            edge: Edge::Top,
            ..PanelSettings::default()
        }
    }

    #[test]
    fn holds_a_change_until_the_ui_connects_then_sends() {
        let inbox = Inbox::default();
        inbox.deliver(PanelSettings::default());
        inbox.deliver(top());
        let signal = Signal::new(PanelSettings::default());
        assert_eq!(inbox.connect(signal), Some(top()));
        inbox.deliver(PanelSettings::default());
        assert_eq!(signal.get(), PanelSettings::default());
        inbox.deliver(top());
        assert_eq!(signal.get(), top());
    }

    #[test]
    fn connecting_with_nothing_pending_gives_nothing() {
        let inbox = Inbox::<PanelSettings>::default();
        assert_eq!(inbox.connect(Signal::new(PanelSettings::default())), None);
    }

    fn dirs(root: &Path) -> BaseDirs {
        let home: OsString = root.join("home").into();
        let system: OsString = root.join("etc").into();
        BaseDirs::from_vars(|name| match name {
            "XDG_CONFIG_HOME" => Some(home.clone()),
            "XDG_CONFIG_DIRS" => Some(system.clone()),
            _ => None,
        })
    }

    fn write(root: &Path, text: &str) {
        let dir = root.join("home/deskrinch");
        std::fs::create_dir_all(&dir).unwrap();
        let new = dir.join("config.toml.new");
        std::fs::write(&new, text).unwrap();
        std::fs::rename(new, dir.join("config.toml")).unwrap();
    }

    #[test]
    fn starts_from_the_file_and_follows_it() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "[panel]\nheight = 48\n");
        let live = LiveSettings::<PanelSettings>::start(&dirs(root.path()));
        assert_eq!(live.initial.height.get(), 48);
        assert!(format!("{live:?}").contains("following: true"));

        write(root.path(), "[panel]\nheight = 48\nedge = \"top\"\n");
        let deadline = Instant::now() + Duration::from_secs(5);
        let newer = loop {
            let pending = {
                let target = live.inbox.0.lock().unwrap();
                match &*target {
                    Target::Waiting(pending) => pending.clone(),
                    Target::Connected(_) => unreachable!("nothing connected"),
                }
            };
            if let Some(newer) = pending {
                break newer;
            }
            assert!(Instant::now() < deadline, "no change delivered");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(newer.edge, Edge::Top);
        assert_eq!(newer.height.get(), 48);
    }

    #[test]
    fn a_bad_file_starts_from_the_defaults() {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "[panel]\nheight = 0\n");
        let live = LiveSettings::<PanelSettings>::start(&dirs(root.path()));
        assert_eq!(live.initial, PanelSettings::default());
    }
}
