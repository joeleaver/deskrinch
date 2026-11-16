//! What `cargo xtask in-session` waits for and does on the session's own
//! bus: a name owned, and a notification sent.

use std::collections::HashMap;

use deskrinch_test_notify::NotificationsProxy;
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

/// Sends one notification, with no timeout, to
/// `org.freedesktop.Notifications`; its id.
pub(super) fn notify(connection: &Connection) -> zbus::Result<u32> {
    zbus::block_on(async {
        let proxy = NotificationsProxy::new(connection).await?;
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
    })
}
