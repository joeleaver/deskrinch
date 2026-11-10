//! What the compositor tests share: which nested compositor they run in,
//! and waiting for events.

pub(crate) mod compositor;
pub(crate) mod test_window;

pub(crate) use compositor::Compositor;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use deskrinch_window_management::{Event, Window, WindowId, WindowManager, connect};

/// How long a test waits for the compositor to report something.
const PATIENCE: Duration = Duration::from_secs(10);

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
}

impl Watcher {
    pub(crate) fn connect() -> Self {
        let (sender, events) = mpsc::channel();
        let manager = connect(move |event| {
            // The receiver outlives the manager in every test.
            sender.send(event).unwrap();
        })
        .expect("connect to the compositor's window management");
        Self {
            manager,
            events,
            seen: RefCell::default(),
        }
    }

    /// Waits until the list the events describe satisfies `check`, folding
    /// in events as they arrive.
    pub(crate) fn wait_until<T>(&self, what: &str, check: impl Fn(&[Window]) -> Option<T>) -> T {
        let deadline = Instant::now() + PATIENCE;
        loop {
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
            Event::Ended(reason) => panic!("the connection ended: {reason}"),
            other => panic!("an event the tests do not know: {other:?}"),
        }
    }
}

/// The window with `app_id`.
pub(crate) fn find<'a>(windows: &'a [Window], app_id: &str) -> Option<&'a Window> {
    windows.iter().find(|window| window.app_id == app_id)
}
