//! The `org.kde.StatusNotifierWatcher` object.

use std::sync::Arc;

use zbus::Connection;
use zbus::fdo;
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::wire::ObjectPath;

use crate::dbus::DEFAULT_ITEM_PATH;
use crate::host::shared::Shared;
use crate::model::Target;

/// The served object; registrations go to the shared state.
pub(crate) struct Watcher {
    pub(crate) shared: Arc<Shared>,
}

// Each call is handled in a task of its own (zbus's default), so a call
// waiting on the bus for a name's owner holds up no other call.
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    /// Registers an item. `service` is the item's bus name, at
    /// `/StatusNotifierItem`, or an object path of the caller's own
    /// connection (libayatana's way). Registering an item again changes
    /// nothing.
    async fn register_status_notifier_item(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        service: &str,
    ) -> fdo::Result<()> {
        let sender = sender(&header)?;
        let target = if service.starts_with('/') {
            ObjectPath::try_from(service).map_err(|_| {
                fdo::Error::InvalidArgs(format!("{service:?} is not an object path"))
            })?;
            Target {
                service: sender.to_string(),
                path: service.to_owned(),
            }
        } else {
            BusName::try_from(service)
                .map_err(|_| fdo::Error::InvalidArgs(format!("{service:?} is not a bus name")))?;
            Target {
                service: service.to_owned(),
                path: DEFAULT_ITEM_PATH.to_owned(),
            }
        };
        let owner = owner(connection, &target.service).await?;
        let registered = self
            .shared
            .register(target, &owner)
            .map_err(|refused| fdo::Error::LimitsExceeded(refused.to_string()))?;
        if let Some(id) = registered {
            // Sending fails only when the connection is gone, and then
            // nobody is left to hear it.
            let _sent = Self::status_notifier_item_registered(&emitter, id.as_str()).await;
        }
        Ok(())
    }

    /// Registers another host. `service` is its bus name; it is
    /// registered until that name has no owner.
    async fn register_status_notifier_host(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        service: &str,
    ) -> fdo::Result<()> {
        sender(&header)?;
        BusName::try_from(service)
            .map_err(|_| fdo::Error::InvalidArgs(format!("{service:?} is not a bus name")))?;
        owner(connection, service).await?;
        let new = self
            .shared
            .register_host(service)
            .map_err(|refused| fdo::Error::LimitsExceeded(refused.to_string()))?;
        if new {
            let _sent = Self::status_notifier_host_registered(&emitter).await;
        }
        Ok(())
    }

    /// Every registered item, as `<service><path>`, whether or not this
    /// host has read it yet.
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.shared.registered()
    }

    /// Always true: this process is a host for as long as it serves the
    /// watcher.
    #[zbus(property)]
    #[expect(
        clippy::unused_self,
        reason = "zbus serves a property through a method of the object"
    )]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    /// The protocol version; 0, as Plasma's watcher reports.
    #[zbus(property)]
    #[expect(
        clippy::unused_self,
        reason = "zbus serves a property through a method of the object"
    )]
    fn protocol_version(&self) -> i32 {
        0
    }

    /// An item registered.
    #[zbus(signal)]
    pub(crate) async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    /// An item left: its service has no owner any more.
    #[zbus(signal)]
    pub(crate) async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    /// A host registered.
    #[zbus(signal)]
    pub(crate) async fn status_notifier_host_registered(
        emitter: &SignalEmitter<'_>,
    ) -> zbus::Result<()>;

    /// A host left.
    #[zbus(signal)]
    pub(crate) async fn status_notifier_host_unregistered(
        emitter: &SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}

fn sender<'h>(header: &'h Header<'_>) -> fdo::Result<&'h UniqueName<'h>> {
    header
        .sender()
        .ok_or_else(|| fdo::Error::Failed("the call has no sender".into()))
}

/// The unique name of the connection that owns `service`; an error when
/// nobody does, so a registration for a service that does not exist is
/// refused.
async fn owner(connection: &Connection, service: &str) -> fdo::Result<String> {
    if service.starts_with(':') {
        let exists: bool = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(service,),
            )
            .await?
            .body()
            .deserialize()?;
        return if exists {
            Ok(service.to_owned())
        } else {
            Err(fdo::Error::ServiceUnknown(format!(
                "{service} is not connected"
            )))
        };
    }
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetNameOwner",
            &(service,),
        )
        .await
        .map_err(|_| fdo::Error::ServiceUnknown(format!("{service} has no owner")))?;
    Ok(reply.body().deserialize::<String>()?)
}
