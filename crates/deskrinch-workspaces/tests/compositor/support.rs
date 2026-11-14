//! What the tests share: the compositor guard, a subscriber that folds the
//! events into the workspaces they describe, and KWin's own D-Bus
//! interface for desktops.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use deskrinch_bus::Bus;
use deskrinch_test_wait::PATIENCE;
use deskrinch_workspaces::{
    Action, Client, EndReason, Event, Workspace, WorkspaceId, WorkspaceManager, WorkspaceState,
    Workspaces, connect, connect_with,
};
use zbus::{OwnedVariant, Variant};

pub(crate) use deskrinch_test_client::Compositor;

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
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(reason) = &*self.ended.borrow() {
                panic!("waiting for {what}: the connection ended: {reason}");
            }
            let workspaces: Vec<_> = self.seen.borrow().values().cloned().collect();
            if let Some(found) = check(&workspaces) {
                return found;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(event) => self.fold(event),
                Err(error) => panic!("waiting for {what}: {error}; seen {workspaces:#?}"),
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

/// KWin's desktops, through its own D-Bus interface on the session's bus.
pub(crate) mod kwin {
    use super::{Bus, OwnedVariant, Variant};

    const SERVICE: &str = "org.kde.KWin";
    const PATH: &str = "/VirtualDesktopManager";
    const INTERFACE: &str = "org.kde.KWin.VirtualDesktopManager";
    const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

    fn connection() -> zbus::Connection {
        zbus::block_on(Bus::Session.connect()).unwrap()
    }

    fn get(property: &str) -> OwnedVariant {
        let reply = zbus::block_on(connection().call_method(
            Some(SERVICE),
            PATH,
            Some(PROPERTIES),
            "Get",
            &(INTERFACE, property),
        ))
        .unwrap_or_else(|error| panic!("KWin's {property}: {error}"));
        reply.body().deserialize::<OwnedVariant>().unwrap()
    }

    fn set(property: &str, value: Variant<'_>) {
        zbus::block_on(connection().call_method(
            Some(SERVICE),
            PATH,
            Some(PROPERTIES),
            "Set",
            &(INTERFACE, property, value),
        ))
        .unwrap_or_else(|error| panic!("setting KWin's {property}: {error}"));
    }

    /// Each desktop's (position, id, name).
    pub(crate) fn desktops() -> Vec<(u32, String, String)> {
        Vec::try_from(get("desktops")).unwrap()
    }

    /// The current desktop's name.
    pub(crate) fn current_name() -> String {
        let current = String::try_from(get("current")).unwrap();
        desktops()
            .into_iter()
            .find(|(_, id, _)| *id == current)
            .map_or_else(
                || panic!("KWin's current desktop {current} is not listed"),
                |(_, _, name)| name,
            )
    }

    /// Makes the desktop named `name` current, as KWin's own interface does.
    pub(crate) fn set_current(name: &str) {
        let id = desktops()
            .into_iter()
            .find(|(_, _, listed)| listed == name)
            .map_or_else(|| panic!("KWin has no desktop {name}"), |(_, id, _)| id);
        set("current", id.as_str().into());
    }

    /// Lays the desktops out in `rows` rows.
    pub(crate) fn set_rows(rows: u32) {
        set("rows", rows.into());
    }

    /// Adds a desktop named `name` at the end.
    pub(crate) fn create(name: &str) {
        let count = u32::try_from(desktops().len()).unwrap();
        zbus::block_on(connection().call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "createDesktop",
            &(count, name),
        ))
        .unwrap_or_else(|error| panic!("KWin's createDesktop: {error}"));
    }

    /// Removes the desktop named `name`.
    pub(crate) fn remove(name: &str) {
        let id = desktops()
            .into_iter()
            .find(|(_, _, listed)| listed == name)
            .map_or_else(|| panic!("KWin has no desktop {name}"), |(_, id, _)| id);
        zbus::block_on(connection().call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "removeDesktop",
            &(id,),
        ))
        .unwrap_or_else(|error| panic!("KWin's removeDesktop: {error}"));
    }

    /// At least [`super::COUNT`] desktops, the first current, in one row.
    /// Returns the first desktop's name.
    pub(crate) fn set_up() -> String {
        while desktops().len() < super::COUNT {
            create(&format!("Desktop {}", desktops().len() + 1));
        }
        set_rows(1);
        let first = desktops()
            .into_iter()
            .min_by_key(|(position, _, _)| *position)
            .map(|(_, _, name)| name)
            .unwrap();
        set_current(&first);
        first
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
            let first = kwin::set_up();
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
