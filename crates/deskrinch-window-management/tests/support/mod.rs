//! What the compositor tests share: which nested compositor they run in,
//! what it does, and waiting for events.

pub(crate) use deskrinch_test_client::Compositor;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use deskrinch_window_management::{
    Client, EndReason, Event, Protocol, Window, WindowId, WindowManager, connect, connect_with,
};

/// How long a test waits for the compositor to report something.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// What each compositor does, where they differ.
pub(crate) trait Behaviour {
    /// The protocol the crate speaks to it.
    fn protocol(self) -> Protocol;
    /// Whether it has a minimized state.
    fn minimizes(self) -> bool;
    /// Whether it maximizes a window that asks.
    fn maximizes(self) -> bool;
    /// Whether it reports a window's parent.
    fn reports_parents(self) -> bool;
    /// Whether a window's reported outputs follow it while it is
    /// fullscreen.
    fn tracks_fullscreen_outputs(self) -> bool;
}

impl Behaviour for Compositor {
    /// The protocol the crate speaks to it.
    fn protocol(self) -> Protocol {
        match self {
            Self::Kwin => Protocol::PlasmaWindowManagement,
            Self::Sway | Self::Labwc => Protocol::WlrForeignToplevel,
        }
    }

    /// Whether it has a minimized state. sway does not: it ignores the
    /// protocol's minimize requests.
    fn minimizes(self) -> bool {
        self != Self::Sway
    }

    /// Whether it maximizes a window that asks. sway tiles every window
    /// and never maximizes one.
    fn maximizes(self) -> bool {
        self != Self::Sway
    }

    /// Whether it reports a window's parent. sway offers the protocol
    /// version that can, but never sets one.
    fn reports_parents(self) -> bool {
        self != Self::Sway
    }

    /// Whether a window's reported outputs follow it while it is
    /// fullscreen. sway reports them from the window's tiled place, which
    /// fullscreen hides, so they catch up only once it is windowed again.
    fn tracks_fullscreen_outputs(self) -> bool {
        self != Self::Sway
    }
}

/// An app id no other window in the session has.
pub(crate) fn unique_app_id(name: &str) -> String {
    format!("org.deskrinch.test.{name}.p{}", std::process::id())
}

/// A connection, and the window list as its events alone describe it: what
/// a taskbar following the events would show.
#[derive(Debug)]
pub(crate) struct Watcher {
    pub(crate) manager: WindowManager,
    events: Receiver<Event>,
    seen: RefCell<BTreeMap<WindowId, Window>>,
    ended: RefCell<Option<EndReason>>,
}

impl Watcher {
    /// A manager on a connection of its own.
    pub(crate) fn connect() -> Self {
        let (sender, events) = mpsc::channel();
        let manager = connect(move |event| {
            // The receiver outlives the manager in every test.
            sender.send(event).unwrap();
        })
        .expect("connect to the compositor's window management");
        Self::watching(manager, events)
    }

    /// A manager on `client`'s connection, which others may share.
    pub(crate) fn connect_with(client: &Client) -> Self {
        let (sender, events) = mpsc::channel();
        let manager = connect_with(client, move |event| {
            // The receiver outlives the manager in every test.
            sender.send(event).unwrap();
        })
        .expect("connect to the compositor's window management");
        Self::watching(manager, events)
    }

    fn watching(manager: WindowManager, events: Receiver<Event>) -> Self {
        Self {
            manager,
            events,
            seen: RefCell::default(),
            ended: RefCell::default(),
        }
    }

    /// Waits until the list the events describe satisfies `check`, folding
    /// in events as they arrive.
    pub(crate) fn wait_until<T>(&self, what: &str, check: impl Fn(&[Window]) -> Option<T>) -> T {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(reason) = &*self.ended.borrow() {
                panic!("waiting for {what}: the connection ended: {reason}");
            }
            let windows: Vec<_> = self.seen.borrow().values().cloned().collect();
            if let Some(found) = check(&windows) {
                return found;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => self.fold(event),
                Err(error) => panic!("waiting for {what}: {error}; the list is {windows:#?}"),
            }
        }
    }

    /// Waits until the window with `app_id` is listed in a state `accept`
    /// takes, and returns it.
    pub(crate) fn wait_window(
        &self,
        what: &str,
        app_id: &str,
        accept: impl Fn(&Window) -> bool,
    ) -> Window {
        self.wait_until(what, |windows| {
            find(windows, app_id)
                .filter(|window| accept(window))
                .cloned()
        })
    }

    /// Waits until no window has `app_id`.
    pub(crate) fn wait_gone(&self, what: &str, app_id: &str) {
        self.wait_until(what, |windows| {
            find(windows, app_id).is_none().then_some(())
        });
    }

    /// Waits for the connection to end, and returns why.
    pub(crate) fn wait_ended(&self, what: &str) -> EndReason {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(reason) = self.ended.borrow().clone() {
                return reason;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => self.fold(event),
                Err(error) => panic!("waiting for {what}: {error}"),
            }
        }
    }

    /// Folds in every event already delivered, without waiting.
    pub(crate) fn drain(&self) {
        while let Ok(event) = self.events.try_recv() {
            self.fold(event);
        }
    }

    /// The list as the events describe it.
    pub(crate) fn seen(&self) -> Vec<Window> {
        self.seen.borrow().values().cloned().collect()
    }

    fn fold(&self, event: Event) {
        assert!(
            self.ended.borrow().is_none(),
            "an event after Ended: {event:?}"
        );
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::Added(window) => {
                let previous = seen.insert(window.id, window);
                assert!(previous.is_none(), "added twice: {previous:?}");
            }
            Event::Changed(window) => {
                let previous = seen.insert(window.id, window);
                assert!(previous.is_some(), "changed before added");
            }
            Event::Removed(id) => {
                assert!(seen.remove(&id).is_some(), "removed before added: {id}");
            }
            Event::Ended(reason) => *self.ended.borrow_mut() = Some(reason),
            other => panic!("an event the tests do not know: {other:?}"),
        }
    }
}

/// The window with `app_id`.
pub(crate) fn find<'a>(windows: &'a [Window], app_id: &str) -> Option<&'a Window> {
    windows.iter().find(|window| window.app_id == app_id)
}
