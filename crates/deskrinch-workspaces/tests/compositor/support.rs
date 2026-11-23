//! What the tests share: the compositor guard, a subscriber that folds the
//! events into the workspaces they describe, and KWin's own D-Bus
//! interface for desktops.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};

use deskrinch_test_wait::{Deadline, waited_in_vain};
use deskrinch_workspaces::{
    Action, Client, EndReason, Event, Workspace, WorkspaceId, WorkspaceManager, WorkspaceState,
    Workspaces, connect, connect_with,
};

pub(crate) use deskrinch_test_client::Compositor;
use deskrinch_test_kwin::KwinActions;

/// KWin in the session this test runs in, for changing its desktops
/// through its own interface (the compositor's side of the KWin tests).
pub(crate) fn kwin() -> KwinActions {
    KwinActions::here().unwrap()
}

/// The workspaces every test has: labwc's sessions configure four, and the
/// KWin tests make four.
pub(crate) const COUNT: usize = 4;

/// The compositor the tests run in, refusing anything but a nested session.
pub(crate) fn compositor() -> Compositor {
    Compositor::current()
}

/// A connection, and the workspaces as its events alone describe them.
#[derive(Debug)]
pub(crate) struct Watcher {
    pub(crate) manager: WorkspaceManager,
    events: Receiver<Event>,
    seen: RefCell<BTreeMap<WorkspaceId, Workspace>>,
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
        .expect("connect to the compositor's workspaces");
        Self::watching(manager, events)
    }

    /// A manager on `client`'s connection.
    pub(crate) fn connect_with(client: &Client) -> Self {
        let (sender, events) = mpsc::channel();
        let manager = connect_with(client, move |event| {
            sender.send(event).unwrap();
        })
        .expect("connect to the compositor's workspaces");
        Self::watching(manager, events)
    }

    fn watching(manager: WorkspaceManager, events: Receiver<Event>) -> Self {
        Self {
            manager,
            events,
            seen: RefCell::default(),
            ended: RefCell::default(),
        }
    }

    /// Waits until the workspaces the events describe satisfy `check`.
    pub(crate) fn wait_until<T>(&self, what: &str, check: impl Fn(&[Workspace]) -> Option<T>) -> T {
        let deadline = Deadline::patience();
        loop {
            if let Some(reason) = &*self.ended.borrow() {
                panic!("waiting for {what}: the connection ended: {reason}");
            }
            let workspaces: Vec<_> = self.seen.borrow().values().cloned().collect();
            if let Some(found) = check(&workspaces) {
                return found;
            }
            match self.events.recv_timeout(deadline.left()) {
                Ok(event) => self.fold(event),
                Err(RecvTimeoutError::Timeout) => panic!("{}", waited_in_vain(what, &workspaces)),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("waiting for {what}: the subscriber is gone; seen {workspaces:#?}")
                }
            }
        }
    }

    /// Waits until the workspace named `name` is the active one.
    pub(crate) fn wait_active(&self, name: &str) -> Workspace {
        self.wait_until(&format!("{name} active"), |workspaces| {
            let active: Vec<_> = workspaces
                .iter()
                .filter(|workspace| workspace.state.contains(WorkspaceState::ACTIVE))
                .collect();
            match active.as_slice() {
                [only] if only.name == name => Some((*only).clone()),
                _ => None,
            }
        })
    }

    fn fold(&self, event: Event) {
        match event {
            Event::Added(workspace) | Event::Changed(workspace) => {
                self.seen.borrow_mut().insert(workspace.id, workspace);
            }
            Event::Removed(id) => {
                self.seen.borrow_mut().remove(&id);
            }
            Event::Ended(reason) => *self.ended.borrow_mut() = Some(reason),
            _ => {}
        }
    }

    /// The workspaces the manager lists, which must match the events'.
    pub(crate) fn listed(&self) -> Vec<Workspace> {
        self.manager.workspaces()
    }

    /// A workspace other than the active one.
    pub(crate) fn inactive(&self) -> Workspace {
        self.wait_until("an inactive workspace", |workspaces| {
            (workspaces.len() >= COUNT)
                .then(|| {
                    workspaces
                        .iter()
                        .find(|workspace| !workspace.state.contains(WorkspaceState::ACTIVE))
                        .cloned()
                })
                .flatten()
        })
    }

    /// Activates the workspace named `name` through the crate, and waits
    /// until it is active.
    pub(crate) fn activate(&self, name: &str) {
        let workspace = self.wait_until(&format!("{name} listed"), |workspaces| {
            workspaces
                .iter()
                .find(|workspace| workspace.name == name)
                .cloned()
        });
        self.manager
            .perform(workspace.id, Action::Activate)
            .expect("request the switch");
        self.wait_active(name);
    }
}

/// The compositor's workspaces as each test starts: at least
/// [`COUNT`], the first active. labwc's sessions have four; KWin's are made
/// with its own interface.
pub(crate) fn set_up(compositor: Compositor, watcher: &Watcher) {
    match compositor {
        Compositor::Kwin => {
            // Made current through KWin's own interface: the watcher sees
            // it only once its events say so, and until then would take
            // an earlier test's desktop for the active one.
            let first = kwin().set_up_virtual_desktops(COUNT).unwrap();
            watcher.wait_active(&first);
        }
        Compositor::Labwc => {
            watcher.wait_until("the workspaces", |workspaces| {
                (workspaces.len() >= COUNT).then_some(())
            });
            let first = watcher.listed()[0].name.clone();
            watcher.activate(&first);
        }
        Compositor::Sway => panic!("sway offers no workspace protocol"),
    }
}
