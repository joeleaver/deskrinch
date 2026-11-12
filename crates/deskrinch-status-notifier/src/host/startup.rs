//! Starting the host: the calls that expect a reply from the bus, made as
//! the tracker's futures once it polls the owners' stream, so that none is
//! awaited while a stream the host holds goes unpolled (the host's module
//! says why that matters).
//!
//! Two stages, so that neither holds a stream while it awaits a reply:
//! [`items`] makes the items' match and hands its stream to the tracker,
//! then [`names`] requests the watcher's name and the host's and announces
//! the host. Both streams exist before the watcher's name is owned, so no
//! registration or departure is missed.

use std::sync::atomic::{AtomicU32, Ordering};

use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::object_server::SignalEmitter;
use zbus::{Connection, MessageStream};

use crate::dbus::watcher::Watcher;
use crate::dbus::{HOST_NAME_PREFIX, WATCHER_NAME, WATCHER_PATH, streams};
use crate::host::StartError;

/// The items' signals.
pub(super) async fn items(connection: Connection) -> Result<MessageStream, StartError> {
    streams::item_signals(&connection)
        .await
        .map_err(StartError::Connect)
}

/// Owns the watcher's name and the host's, and tells items already waiting
/// for a host of this one.
pub(super) async fn names(connection: Connection) -> Result<(), StartError> {
    if !own(&connection, WATCHER_NAME).await? {
        return Err(StartError::NameTaken);
    }
    let host_name = host_name();
    if !own(&connection, &host_name).await? {
        return Err(StartError::HostNameTaken(host_name));
    }
    let emitter = SignalEmitter::new(&connection, WATCHER_PATH).map_err(StartError::Connect)?;
    // It fails only when the connection is gone, which the tracker reports.
    let _sent = Watcher::status_notifier_host_registered(&emitter).await;
    Ok(())
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
