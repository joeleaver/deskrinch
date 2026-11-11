//! The signals the host follows: names changing owner, and items and
//! menus saying that something changed.

use zbus::message::Type;
use zbus::names::MemberName;
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::dbus::{ITEM_INTERFACE, MENU_INTERFACE};

/// Every `NameOwnerChanged` on the bus, opened before the watcher's name is
/// requested so that no departure is missed.
pub(crate) async fn owner_changes(connection: &Connection) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .path("/org/freedesktop/DBus")
        .member("NameOwnerChanged")
        .build()?;
    MessageStream::for_match_rule(rule, connection, None).await
}

/// Every signal of the items' interface, from anyone.
pub(crate) async fn item_signals(connection: &Connection) -> zbus::Result<MessageStream> {
    signals(connection, ITEM_INTERFACE).await
}

/// Every signal of the menus' interface, from anyone: the ones of menus
/// that are not open are dropped as they come.
pub(crate) async fn menu_signals(connection: &Connection) -> zbus::Result<MessageStream> {
    signals(connection, MENU_INTERFACE).await
}

async fn signals(connection: &Connection, interface: &'static str) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(interface)
        .build()?;
    MessageStream::for_match_rule(rule, connection, None).await
}

/// A `NameOwnerChanged`: the name, and its new owner (`None` for nobody).
pub(crate) fn owner_change(message: &Message) -> Option<(String, Option<String>)> {
    let header = message.header();
    if header.member().map(MemberName::as_str) != Some("NameOwnerChanged") {
        return None;
    }
    let (name, _old, new): (String, String, String) = message.body().deserialize().ok()?;
    Some((name, Some(new).filter(|n| !n.is_empty())))
}

/// An item's signal that its properties changed: its sender and path.
pub(crate) fn item_signal(message: &Message) -> Option<(String, String)> {
    let header = message.header();
    let member = header.member()?.as_str();
    let changed = matches!(
        member,
        "NewTitle"
            | "NewIcon"
            | "NewAttentionIcon"
            | "NewOverlayIcon"
            | "NewMenu"
            | "NewToolTip"
            | "NewStatus"
    );
    if !changed {
        return None;
    }
    Some((header.sender()?.to_string(), header.path()?.to_string()))
}

/// A menu's signal that its layout or its entries changed: its sender and
/// path. `ItemActivationRequested`, an application asking for its menu to
/// be shown, is not followed.
pub(crate) fn menu_signal(message: &Message) -> Option<(String, String)> {
    let header = message.header();
    let member = header.member()?.as_str();
    if !matches!(member, "LayoutUpdated" | "ItemsPropertiesUpdated") {
        return None;
    }
    Some((header.sender()?.to_string(), header.path()?.to_string()))
}
