//! The running service: its configuration, the events it reports, and the
//! handle the UI acts through.

pub(crate) mod shared;
mod worker;

use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::Connection;
use zbus::fdo::RequestNameReply;

use crate::capabilities::Capabilities;
use crate::clock::Clock;
use crate::dbus::interface::{NAME, Notifications, PATH};
use crate::dbus::name::{self, NameRequest, NameStatus, Watcher};
use crate::limits::Limits;
use crate::notification::{CloseReason, Notification, NotificationId};
use crate::request::Policy;
use shared::Shared;

/// Which bus the service connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The session bus, from `DBUS_SESSION_BUS_ADDRESS`.
    Session,
    /// The bus at this D-Bus address, such as `unix:path=/run/…/bus`.
    Address(String),
}

/// What `GetServerInformation` reports, besides the specification version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    /// The product name.
    pub name: String,
    /// The vendor.
    pub vendor: String,
    /// The product version.
    pub version: String,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: "deskrinch".into(),
            vendor: "deskrinch".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// How to start the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The bus to serve on.
    pub bus: Bus,
    /// How to request the name.
    pub name: NameRequest,
    /// What `GetServerInformation` reports.
    pub server: ServerInfo,
    /// The optional capabilities to advertise, and follow.
    pub capabilities: Capabilities,
    /// The bounds on notifications and senders.
    pub limits: Limits,
    /// The timeout of a notification that leaves it to the server; `None`
    /// keeps such notifications until dismissed.
    pub default_timeout: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bus: Bus::Session,
            name: NameRequest::default(),
            server: ServerInfo::default(),
            capabilities: Capabilities::default(),
            limits: Limits::default(),
            default_timeout: Some(Duration::from_secs(5)),
        }
    }
}

/// A change the subscriber hears of, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// A new notification is live, after the others.
    Shown(Arc<Notification>),
    /// A live notification's content was replaced, keeping its id and place.
    Replaced(Arc<Notification>),
    /// A notification left: it expired, was dismissed, or the sender closed
    /// it.
    Closed {
        /// The notification.
        id: NotificationId,
        /// Why.
        reason: CloseReason,
    },
    /// The service now owns the name (after waiting in the queue, or taking
    /// it back).
    NameAcquired,
    /// Another service took the name; notifications no longer reach this one.
    NameLost,
    /// The bus connection ended. The service receives nothing more; the
    /// daemon decides whether to start another.
    Disconnected,
}

/// Why the service could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// Connecting to the bus, or serving the object on it, failed.
    #[error("could not connect to the bus")]
    Connect(#[source] zbus::Error),
    /// Another service owns `org.freedesktop.Notifications` and did not let
    /// it go (or `replace_existing` was not asked for), and queueing was not
    /// asked for.
    #[error("another service owns {NAME}")]
    NameTaken,
    /// Requesting the name failed for another reason.
    #[error("could not request {NAME}")]
    RequestName(#[source] zbus::Error),
    /// A thread of the service could not be started.
    #[error("could not start a service thread")]
    Thread(#[source] io::Error),
}

/// Why an operation on a [`Handle`] did nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OperationError {
    /// The notification is not live (it may just have expired or closed).
    #[error("notification {0} is not live")]
    NotFound(NotificationId),
    /// The notification has no action with this key.
    #[error("notification {id} has no action {key:?}")]
    UnknownAction {
        /// The notification.
        id: NotificationId,
        /// The key asked for.
        key: String,
    },
    /// The service has stopped.
    #[error("the notification service has stopped")]
    Stopped,
}

/// The running service. Dropping it stops it and releases the name.
///
/// Dropping, like [`Service::start`], blocks the calling thread on the bus,
/// so neither may happen inside an async runtime's task.
pub struct Service {
    shared: Arc<Shared>,
    status: NameStatus,
    watcher: Option<Watcher>,
    worker: Option<JoinHandle<()>>,
    connection: Option<Connection>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Service")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Service {
    /// Connects to the bus, serves the object, requests the name, and starts
    /// reporting events to `subscriber`, on a thread of the service's own.
    ///
    /// # Errors
    ///
    /// [`StartError::NameTaken`] when the name is owned by another service
    /// and `config.name` neither takes it over nor queues for it; the other
    /// variants for failures of the bus or the system.
    pub fn start(
        config: Config,
        clock: Arc<dyn Clock>,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<Self, StartError> {
        let policy = Policy {
            limits: config.limits,
            capabilities: config.capabilities,
            default_timeout: config.default_timeout,
        };
        let shared = Shared::new(policy, clock, config.server);
        let object = Notifications {
            shared: Arc::clone(&shared),
        };
        let flags = config.name.flags();
        let (connection, signals, status) = zbus::block_on(async {
            let builder = match &config.bus {
                Bus::Session => zbus::connection::Builder::session(),
                Bus::Address(address) => zbus::connection::Builder::address(address.as_str()),
            };
            let connection = builder
                .serve_at(PATH, object)
                .build()
                .await
                .map_err(StartError::Connect)?;
            let signals = name::name_signals(&connection)
                .await
                .map_err(StartError::Connect)?;
            let reply = connection
                .request_name_with_flags(NAME, flags)
                .await
                .map_err(|e| match e {
                    zbus::Error::NameTaken => StartError::NameTaken,
                    e => StartError::RequestName(e),
                })?;
            let status = match reply {
                RequestNameReply::InQueue => NameStatus::Queued,
                RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {
                    NameStatus::Owned
                }
                RequestNameReply::Exists => return Err(StartError::NameTaken),
            };
            Ok((connection, signals, status))
        })?;
        let owned = status == NameStatus::Owned;
        shared.set_owns_name(owned);

        let mut service = Self {
            shared: Arc::clone(&shared),
            status,
            watcher: None,
            worker: None,
            connection: Some(connection.clone()),
        };
        let worker_shared = Arc::clone(&shared);
        let subscriber: worker::Subscriber = Box::new(subscriber);
        service.worker = Some(
            thread::Builder::new()
                .name("notifications".into())
                .spawn(move || worker::run(&worker_shared, &connection, &subscriber))
                .map_err(StartError::Thread)?,
        );
        service.watcher = Some(Watcher::spawn(signals, shared, owned).map_err(StartError::Thread)?);
        Ok(service)
    }

    /// Whether the service owned the name or was queued for it when it
    /// started. Later changes come as [`Event::NameAcquired`] and
    /// [`Event::NameLost`]; [`Handle::owns_name`] tells the current state.
    #[must_use]
    pub fn initial_status(&self) -> NameStatus {
        self.status
    }

    /// A handle to act on the service from any thread.
    #[must_use]
    pub fn handle(&self) -> Handle {
        Handle {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        // The watcher holds a stream on the connection; it goes first.
        self.watcher = None;
        self.shared.stop();
        if let Some(worker) = self.worker.take() {
            // The worker only ends by returning; a panic in it (in the
            // subscriber) already reported itself.
            let _joined = worker.join();
        }
        if let Some(connection) = self.connection.take() {
            // Closing releases the name. It fails only when the connection is
            // already gone, which releases it too.
            let _closed = zbus::block_on(connection.close());
        }
    }
}

/// A cheap, cloneable handle for the UI's operations, safe to use from any
/// thread, the subscriber included.
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
    /// The live notifications, in the order they were first shown.
    #[must_use]
    pub fn notifications(&self) -> Vec<Arc<Notification>> {
        self.shared.notifications()
    }

    /// The live notification `id`.
    #[must_use]
    pub fn notification(&self, id: NotificationId) -> Option<Arc<Notification>> {
        self.shared.notification(id)
    }

    /// The user dismissed `id`: it closes with [`CloseReason::Dismissed`].
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when it is not live,
    /// [`OperationError::Stopped`] after the service stopped.
    pub fn dismiss(&self, id: NotificationId) -> Result<(), OperationError> {
        self.shared.close(id, CloseReason::Dismissed)
    }

    /// Stops `id`'s timer, keeping the time it has left, while the user looks
    /// at it (the pointer is over its popup, say). It stays paused through a
    /// replacement, with its new full timeout left, until
    /// [`resume`](Self::resume). Pausing twice, or pausing a notification
    /// that never expires, changes nothing. A paused notification is closed
    /// like any other: by the user, the sender, or the service stopping.
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when it is not live,
    /// [`OperationError::Stopped`] after the service stopped.
    pub fn pause(&self, id: NotificationId) -> Result<(), OperationError> {
        self.shared.set_paused(id, true)
    }

    /// Starts `id`'s timer again with the time it had left when paused.
    /// Resuming one that is not paused changes nothing.
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when it is not live,
    /// [`OperationError::Stopped`] after the service stopped.
    pub fn resume(&self, id: NotificationId) -> Result<(), OperationError> {
        self.shared.set_paused(id, false)
    }

    /// The user invoked the action `key` on `id` (use
    /// [`Action::DEFAULT_KEY`](crate::Action::DEFAULT_KEY) for activating the
    /// notification itself). The sender gets `ActivationToken` first when
    /// `activation_token` is given (an xdg-activation token the UI obtained
    /// for the sender's window), then `ActionInvoked`. Unless the
    /// notification is resident, it then closes as dismissed.
    ///
    /// # Errors
    ///
    /// [`OperationError::NotFound`] when it is not live,
    /// [`OperationError::UnknownAction`] when it has no such action,
    /// [`OperationError::Stopped`] after the service stopped.
    pub fn invoke_action(
        &self,
        id: NotificationId,
        key: &str,
        activation_token: Option<String>,
    ) -> Result<(), OperationError> {
        self.shared.invoke_action(id, key, activation_token)
    }

    /// Whether the service owns the name now.
    #[must_use]
    pub fn owns_name(&self) -> bool {
        self.shared.owns_name.load(Ordering::SeqCst)
    }
}
