//! What `cargo xtask in-session` waits for and does on the session's own
//! bus: a name owned, and a notification sent.

use std::collections::HashMap;
use std::time::Duration;

use deskrinch_test_notify::{NotificationsProxy, Signal, Signals};
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// A connection to the session's bus (`DBUS_SESSION_BUS_ADDRESS`, which
/// `cargo xtask session` points at its own bus).
pub(super) fn connect() -> zbus::Result<Connection> {
    zbus::block_on(Connection::session())
}

/// Whether `name` has an owner on `connection`'s bus.
pub(super) fn owned(connection: &Connection, name: &str) -> zbus::Result<bool> {
    let name = BusName::try_from(name)?;
    zbus::block_on(async {
        let proxy = DBusProxy::new(connection).await?;
        Ok(proxy.name_has_owner(name).await?)
    })
}

/// A notification sent, and the signals its sender's connection receives:
/// with the service's sender-only delivery, the ones about it.
pub(super) struct Sent {
    signals: Signals,
    id: u32,
    /// Held so that the signals keep coming.
    _connection: Connection,
}

impl Sent {
    /// The reasons `NotificationClosed` gave for it so far, in order: the
    /// signals already received, each waited for at most [`DRAIN`].
    pub(super) fn closed(&self) -> Vec<u32> {
        let mut reasons = Vec::new();
        while let Some(signal) = self.signals.try_next(DRAIN) {
            if let Signal::Closed(id, reason) = signal
                && id == self.id
            {
                reasons.push(reason);
            }
        }
        reasons
    }
}

/// How long [`Sent::closed`] waits for a signal still on its way: the
/// component has ended by then, and what it sent is already on the bus.
const DRAIN: Duration = Duration::from_millis(500);

/// Sends one notification, with no timeout, to
/// `org.freedesktop.Notifications`, listening for its signals from before
/// it is sent.
pub(super) fn notify(connection: Connection) -> zbus::Result<Sent> {
    let signals = Signals::on(&connection);
    let id = zbus::block_on(async {
        let proxy = NotificationsProxy::new(&connection).await?;
        proxy
            .notify(
                "in-session",
                0,
                "",
                "Shown",
                "a popup",
                &[],
                &HashMap::new(),
                0,
            )
            .await
    })?;
    Ok(Sent {
        signals,
        id,
        _connection: connection,
    })
}
