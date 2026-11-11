//! The session-bus plumbing shared by systemd scopes and D-Bus activation.

use std::time::Duration;

use zbus::Connection;

/// Connects to the bus at `address`, with `method_timeout` on every call.
pub(super) async fn connect(address: &str, method_timeout: Duration) -> zbus::Result<Connection> {
    zbus::connection::Builder::address(address)
        .method_timeout(method_timeout)
        .build()
        .await
}

/// Whether some connection owns `name` on the bus.
pub(super) async fn name_has_owner(connection: &Connection, name: &str) -> zbus::Result<bool> {
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &name,
        )
        .await?;
    reply.body().deserialize()
}
