//! The running watcher and host: its configuration, the events it reports,
//! and the handle the tray acts through.

pub(crate) mod shared;
mod tracker;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};

use deskrinch_bus::Bus;
use deskrinch_timer::Timer;
use zbus::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::object_server::SignalEmitter;

use crate::dbus::watcher::Watcher;
use crate::dbus::{HOST_NAME_PREFIX, WATCHER_NAME, WATCHER_PATH, streams};
use crate::item::{Item, ItemId};
use crate::limits::Limits;
use crate::menu::{EntryId, Menu, MenuFailure};
use crate::model::{Change, Model};
use shared::Shared;
use tracker::Tracker;

/// How to start the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The bus to serve on.
    pub bus: Bus,
    /// The bounds on items, connections and reads.
    pub limits: Limits,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bus: Bus::Session,
            limits: Limits::default(),
        }
    }
}

/// A change the subscriber hears of, in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// An item was read for the first time: it is now in the tray, after
    /// the others.
    Added(Arc<Item>),
    /// A reported item's properties changed.
    Changed(Arc<Item>),
    /// A reported item left: its service has no owner any more. Its menu,
    /// if it was open, is closed with it.
    Removed(ItemId),
    /// An open menu was read for the first time, or read again and found
    /// different ([`Handle::open_menu`]).
    Menu(Arc<Menu>),
    /// An open menu could not be read the first time, or the item no longer
    /// has one: the menu is closed, and the UI closes it too. A later read
    /// that fails keeps what was last read, and says nothing.
    MenuFailed {
        /// The item whose menu it was.
        item: ItemId,
        /// Why.
        reason: MenuFailure,
    },
    /// The bus connection ended. The host receives nothing more, and the
    /// items it reported are no longer followed; the daemon decides whether
    /// to start another.
    Disconnected,
}

impl From<Change> for Event {
    fn from(change: Change) -> Self {
        match change {
            Change::Added(item) => Self::Added(item),
            Change::Changed(item) => Self::Changed(item),
            Change::Removed(id) => Self::Removed(id),
            Change::Menu(menu) => Self::Menu(menu),
            Change::MenuFailed(item, reason) => Self::MenuFailed { item, reason },
        }
    }
}

/// Why the host could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// Connecting to the bus, or serving the watcher on it, failed.
    #[error("could not connect to the bus")]
    Connect(#[source] zbus::Error),
    /// Another watcher owns `org.kde.StatusNotifierWatcher` (Plasma's, or
    /// another deskrinch host); this one does not queue for it.
    #[error("another watcher owns {WATCHER_NAME}")]
    NameTaken,
    /// Another connection owns the name this host was to register under,
    /// `org.kde.StatusNotifierHost-<pid>-<n>`.
    #[error("another connection owns {0}")]
    HostNameTaken(String),
    /// Requesting a name failed for another reason.
    #[error("could not request the watcher's or the host's name")]
    RequestName(#[source] zbus::Error),
    /// A thread of the host could not be started.
    #[error("could not start a host thread")]
    Thread(#[source] io::Error),
    /// The timer that paces the reads could not be started.
    #[error("could not start the host's timer")]
    Timer(#[source] deskrinch_timer::StartError),
}

/// Why an operation on a [`Handle`] did nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OperationError {
    /// No reported item has this id (it may just have left).
    #[error("no item {0}")]
    NotFound(ItemId),
    /// The item exports no menu, or its menu is not open.
    #[error("item {0} has no open menu")]
    NoMenu(ItemId),
    /// The open menu has no entry that is still the one this id names (the
    /// menu changed since), or the entry cannot do this: a separator or a
    /// disabled entry cannot be activated, and only an entry with a submenu
    /// opens one.
    #[error("menu entry {0} is not there, or cannot do this")]
    EntryUnavailable(EntryId),
    /// The host has stopped, or its connection has ended.
    #[error("the status notifier host has stopped")]
    Stopped,
}

/// What the user did to an item, with the position on screen to show
/// anything the item opens at (the specification's `x` and `y`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// The primary action, usually a left click (`Activate`).
    Primary {
        /// Horizontal position on screen.
        x: i32,
        /// Vertical position on screen.
        y: i32,
    },
    /// The secondary action, usually a middle click (`SecondaryActivate`).
    Secondary {
        /// Horizontal position on screen.
        x: i32,
        /// Vertical position on screen.
        y: i32,
    },
    /// Asks the item to show its own context menu (`ContextMenu`), for an
    /// item without an exported menu.
    ContextMenu {
        /// Horizontal position on screen.
        x: i32,
        /// Vertical position on screen.
        y: i32,
    },
    /// The wheel turned over the item (`Scroll`).
    Scroll {
        /// How far, in the wheel's steps (positive is down or right).
        delta: i32,
        /// Which way.
        orientation: Orientation,
    },
}

/// The direction of a [`Activation::Scroll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Left and right.
    Horizontal,
    /// Up and down.
    Vertical,
}

impl Orientation {
    /// As the specification spells it on the bus: lowercase, as KDE's
    /// implementation sends it and ksni reads it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// The running watcher and host.
///
/// Dropping it stops following the items, closes the connection, and so
/// gives up both names; items see the watcher leave and, as the
/// specification asks, register again with the next one. Dropping, like
/// [`Host::start`], blocks the calling thread on the bus, so neither may
/// happen inside an async runtime's task.
pub struct Host {
    shared: Arc<Shared>,
    tracker: Option<Tracker>,
    worker: Option<JoinHandle<()>>,
    connection: Connection,
}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").finish_non_exhaustive()
    }
}

impl Host {
    /// Connects to the bus, serves the watcher, owns its name and a host
    /// name, and starts reporting events to `subscriber`, on a thread of
    /// the host's own.
    ///
    /// # Errors
    ///
    /// [`StartError::NameTaken`] when another watcher runs on the bus; the
    /// other variants for failures of the bus or the system.
    pub fn start(
        config: Config,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<Self, StartError> {
        let limits = config.limits;
        let timer = Timer::start().map_err(StartError::Timer)?;
        let shared = Shared::new(Model::new(limits.clone()));
        let watcher = Watcher {
            shared: Arc::clone(&shared),
        };
        let (connection, owners, signals) = zbus::block_on(async {
            let connection = config
                .bus
                .builder()
                .method_timeout(limits.read_timeout)
                .serve_at(WATCHER_PATH, watcher)
                .build()
                .await
                .map_err(StartError::Connect)?;
            let owners = streams::owner_changes(&connection)
                .await
                .map_err(StartError::Connect)?;
            let signals = streams::item_signals(&connection)
                .await
                .map_err(StartError::Connect)?;
            if !own(&connection, WATCHER_NAME).await? {
                return Err(StartError::NameTaken);
            }
            let host_name = host_name();
            if !own(&connection, &host_name).await? {
                return Err(StartError::HostNameTaken(host_name));
            }
            let emitter =
                SignalEmitter::new(&connection, WATCHER_PATH).map_err(StartError::Connect)?;
            // Items already waiting for a host learn of this one; it fails
            // only when the connection is gone, which the tracker reports.
            let _sent = Watcher::status_notifier_host_registered(&emitter).await;
            Ok((connection, owners, signals))
        })?;
        let mut host = Self {
            shared: Arc::clone(&shared),
            tracker: None,
            worker: None,
            connection: connection.clone(),
        };
        let worker_shared = Arc::clone(&shared);
        host.worker = Some(
            thread::Builder::new()
                .name("status-notifier".into())
                .spawn(move || {
                    while let Some(events) = worker_shared.next_events() {
                        events.into_iter().for_each(&subscriber);
                    }
                })
                .map_err(StartError::Thread)?,
        );
        host.tracker = Some(
            Tracker::spawn(connection, owners, signals, shared, limits, timer)
                .map_err(StartError::Thread)?,
        );
        Ok(host)
    }

    /// A handle to act on the host from any thread.
    #[must_use]
    pub fn handle(&self) -> Handle {
        Handle {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // The tracker holds streams on the connection; it goes first.
        self.tracker = None;
        self.shared.stop();
        if let Some(worker) = self.worker.take() {
            // The worker only ends by returning; a panic in it (in the
            // subscriber) already reported itself.
            let _joined = worker.join();
        }
        // Closing releases both names, and ends the handles' use of the
        // connection. It fails only when the connection is already gone,
        // which releases them too.
        let _closed = zbus::block_on(self.connection.clone().close());
    }
}

/// Requests `name`, without queueing or taking it over; whether it is now
/// this connection's.
async fn own(connection: &Connection, name: &str) -> Result<bool, StartError> {
    let reply = connection
        .request_name_with_flags(name, RequestNameFlags::DO_NOT_QUEUE)
        .await;
    match reply {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(true),
        Ok(RequestNameReply::Exists | RequestNameReply::InQueue) | Err(zbus::Error::NameTaken) => {
            Ok(false)
        }
        Err(e) => Err(StartError::RequestName(e)),
    }
}

/// `org.kde.StatusNotifierHost-<pid>-<n>`: Plasma's form, with a count so
/// that several hosts in one process (tests, on buses of their own) differ.
fn host_name() -> String {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    format!("{HOST_NAME_PREFIX}-{}-{n}", std::process::id())
}

/// A cheap, cloneable handle for the tray's operations, safe to use from
/// any thread, the subscriber included.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle").finish_non_exhaustive()
    }
}

impl Handle {
    /// The reported items, in the order they registered.
    #[must_use]
    pub fn items(&self) -> Vec<Arc<Item>> {
        self.shared.items()
    }

    /// The reported item `id`.
    #[must_use]
    pub fn item(&self, id: &ItemId) -> Option<Arc<Item>> {
        self.shared.item(id)
    }

    /// Sends `activation` to the item `id`. With `activation_token` (an
    /// xdg-activation token the tray obtained for the item to raise a
    /// window with), the item gets `ProvideXdgActivationToken` first. Items
    /// without that method ignore it, as they do in Plasma, which sends it
    /// the same way.
    ///
    /// It never waits on the bus: the call is queued for the host's own
    /// thread, which sends the calls in the order they were queued and waits
    /// for no reply. A UI thread may call it.
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when no reported item has this id,
    /// [`OperationError::Stopped`] after the host stopped or its connection
    /// ended.
    pub fn activate(
        &self,
        id: &ItemId,
        activation: Activation,
        activation_token: Option<&str>,
    ) -> Result<(), OperationError> {
        self.shared.activate(id, activation, activation_token)
    }

    /// Opens the item `id`'s menu: the application hears that it opened
    /// (`Event` `opened`) and is asked to get it ready (`AboutToShow`),
    /// then the menu is read, and the subscriber hears of it as
    /// [`Event::Menu`], or [`Event::MenuFailed`]. Until it is closed, the
    /// menu is read again whenever the application says it changed, and
    /// each change is another [`Event::Menu`]. Nothing when it is open
    /// already. It never waits on the bus.
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when no reported item has this id,
    /// [`OperationError::NoMenu`] when the item exports no menu,
    /// [`OperationError::Stopped`] after the host stopped.
    pub fn open_menu(&self, id: &ItemId) -> Result<(), OperationError> {
        self.shared.open_menu(id)
    }

    /// Closes the item `id`'s menu: the application hears that it closed,
    /// the menu is no longer followed, and what was read of it is
    /// released. Nothing when it is not open. It never waits on the bus.
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when no reported item has this id,
    /// [`OperationError::Stopped`] after the host stopped.
    pub fn close_menu(&self, id: &ItemId) -> Result<(), OperationError> {
        self.shared.close_menu(id)
    }

    /// The item `id`'s open menu, as last read; `None` when it is not open
    /// or has not been read yet.
    #[must_use]
    pub fn menu(&self, id: &ItemId) -> Option<Arc<Menu>> {
        self.shared.menu(id)
    }

    /// The submenu of `entry`, in the item `id`'s open menu, opened: the
    /// application hears so and is asked to get it ready (`AboutToShow`).
    /// Applications that fill a submenu only then say that the menu
    /// changed, and it is read again ([`Event::Menu`]). It never waits on
    /// the bus.
    ///
    /// # Errors
    ///
    /// [`OperationError::EntryUnavailable`] when the menu has no entry that
    /// is still `entry`, or it has no submenu; the others as for
    /// [`Handle::open_menu`].
    pub fn open_submenu(&self, id: &ItemId, entry: EntryId) -> Result<(), OperationError> {
        self.shared.open_submenu(id, entry)
    }

    /// The submenu of `entry`, in the item `id`'s open menu, closed: the
    /// application hears so. It never waits on the bus.
    ///
    /// # Errors
    ///
    /// As for [`Handle::open_submenu`].
    pub fn close_submenu(&self, id: &ItemId, entry: EntryId) -> Result<(), OperationError> {
        self.shared.close_submenu(id, entry)
    }

    /// Activates `entry` of the item `id`'s open menu (`Event` `clicked`),
    /// only if the menu still has that entry as the caller was given it:
    /// an entry that changed since, or another one now at its number, is
    /// refused rather than activated. With `activation_token`, the item
    /// gets `ProvideXdgActivationToken` first, as for
    /// [`Handle::activate`]. It never waits on the bus.
    ///
    /// # Errors
    ///
    /// [`OperationError::EntryUnavailable`] when the menu has no entry that
    /// is still `entry`, or it is a separator or disabled;
    /// [`OperationError::NoMenu`] when the menu is not open; the others as
    /// for [`Handle::open_menu`].
    pub fn activate_entry(
        &self,
        id: &ItemId,
        entry: EntryId,
        activation_token: Option<&str>,
    ) -> Result<(), OperationError> {
        self.shared.activate_entry(id, entry, activation_token)
    }
}
