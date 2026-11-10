//! The panel's configuration while it runs.
//!
//! [`Config::start`] starts the process's one configuration [`Watcher`]
//! and subscribes the panel's [section](crate::settings::section): the
//! settings it returns make the window, and every later change reaches the
//! UI through an [`Inbox`]. Diagnostics are written to standard error as
//! they come, with their file, line and column; a broken file leaves the
//! last good settings in place (the configuration crate's rule).

use std::sync::{Arc, Mutex, PoisonError};

use deskrinch_config::{Event, Sources, Subscription, Watcher, load};
use deskrinch_xdg::BaseDirs;
use rinch::prelude::Signal;

use crate::log;
use crate::settings::{self, PanelSettings};

/// The running configuration: kept for as long as the panel runs.
pub(crate) struct Config {
    /// The settings when the panel started, which its window is made with.
    pub(crate) initial: PanelSettings,
    inbox: Arc<Inbox>,
    // The subscription before the watcher: deliveries stop, then the
    // watcher's thread.
    _subscription: Option<Subscription>,
    _watcher: Option<Watcher>,
}

impl Config {
    /// Reads the panel's settings from the configuration in `dirs` and
    /// follows them. When the files cannot be watched (which is logged), the
    /// settings are read once and do not follow changes.
    pub(crate) fn start(dirs: &BaseDirs) -> Self {
        let sources = Sources::new(dirs);
        let section = match settings::section() {
            Ok(section) => section,
            Err(error) => {
                log::error("the settings' defaults", &error);
                return Self::fixed(PanelSettings::default());
            }
        };
        let inbox = Arc::new(Inbox::default());
        let watcher = match Watcher::start(sources.clone(), |error| {
            log::error("watching the configuration", &error);
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                log::error("watching the configuration (it is read once)", &error);
                let resolved = load(&sources, section);
                resolved.diagnostics.iter().for_each(log::diagnostic);
                return Self::fixed(resolved.settings);
            }
        };
        let deliver = Arc::clone(&inbox);
        let (subscription, resolved) = watcher.subscribe(section, move |event| match event {
            Event::Changed(settings) => deliver.deliver(settings),
            Event::Diagnostics(diagnostics) => diagnostics.iter().for_each(log::diagnostic),
        });
        resolved.diagnostics.iter().for_each(log::diagnostic);
        Self {
            initial: resolved.settings,
            inbox,
            _subscription: Some(subscription),
            _watcher: Some(watcher),
        }
    }

    fn fixed(initial: PanelSettings) -> Self {
        Self {
            initial,
            inbox: Arc::default(),
            _subscription: None,
            _watcher: None,
        }
    }

    /// Sends every later change to `signal`, and returns the newest change
    /// that came before, if any.
    pub(crate) fn connect(&self, signal: Signal<PanelSettings>) -> Option<PanelSettings> {
        self.inbox.connect(signal)
    }
}

/// Where the watcher's thread hands the settings to the UI: held until the
/// UI's signal exists, then sent to it with [`Signal::send`], rinch's write
/// from another thread.
#[derive(Debug, Default)]
struct Inbox(Mutex<Target>);

#[derive(Debug)]
enum Target {
    /// No signal yet; the newest change so far.
    Waiting(Option<PanelSettings>),
    /// The UI's signal.
    Connected(Signal<PanelSettings>),
}

impl Default for Target {
    fn default() -> Self {
        Self::Waiting(None)
    }
}

impl Inbox {
    fn deliver(&self, settings: PanelSettings) {
        // A panic while holding the lock leaves a valid `Target`.
        let mut target = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match &mut *target {
            Target::Waiting(pending) => *pending = Some(settings),
            Target::Connected(signal) => signal.send(settings),
        }
    }

    fn connect(&self, signal: Signal<PanelSettings>) -> Option<PanelSettings> {
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
    use crate::settings::Edge;

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
        let inbox = Inbox::default();
        assert_eq!(inbox.connect(Signal::new(PanelSettings::default())), None);
    }
}
