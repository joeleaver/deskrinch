//! Items registering, changing and leaving, as the host reports them and as
//! the watcher announces them.

use std::collections::HashMap;
use std::sync::Arc;

use deskrinch_status_notifier::{Item, Status};

use crate::common::{PrivateBus, WatcherSignals, error_name, start, watcher_property};
use crate::stand_in::{Reply, StandIn, properties, v};

const NAME: &str = "org.kde.StatusNotifierItem-1-1";

#[test]
fn an_item_registered_by_its_name_is_read_and_reported() {
    let bus = PrivateBus::start();
    let (host, events) = start(&bus);
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(properties("A")),
    );
    item.own(NAME);
    item.register(NAME).unwrap();

    let added = events.added();
    assert_eq!(added.id.as_str(), format!("{NAME}/StatusNotifierItem"));
    assert_eq!(added.title, "A");
    assert_eq!(added.app_id, "stand-in");
    assert_eq!(added.status, Status::Active);
    assert_eq!(added.icon.name.as_ref().unwrap().as_str(), "mail-unread");
    assert!(added.has_menu());
    assert_eq!(host.handle().items(), std::slice::from_ref(&added));
    assert_eq!(host.handle().item(&added.id), Some(added));
}

#[test]
fn an_item_registered_by_a_path_is_its_connection_s_libayatana_style() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let path = "/org/ayatana/NotificationItem/app";
    let mut sent = properties("A");
    sent.insert(
        "Menu".into(),
        v(zbus::wire::ObjectPath::try_from("/").unwrap()),
    );
    let item = StandIn::start(&bus, path, Reply::Properties(sent));
    item.register(path).unwrap();

    let added = events.added();
    assert_eq!(added.id.as_str(), format!("{}{path}", item.unique_name()));
    assert!(!added.has_menu());
}

#[test]
fn a_change_signal_reads_the_item_again() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(properties("A")),
    );
    item.register(&item.unique_name()).unwrap();
    events.added();

    item.set(Reply::Properties(properties("B")));
    item.signal("NewTitle");
    assert_eq!(events.changed().title, "B");

    // A signal that changes nothing reports nothing.
    item.signal("NewIcon");
    events.quiet();

    let mut attention = properties("B");
    attention.insert("Status".into(), v("NeedsAttention"));
    item.set(Reply::Properties(attention));
    item.signal("NewStatus");
    assert_eq!(events.changed().status, Status::NeedsAttention);
}

#[test]
fn an_item_that_leaves_is_removed() {
    let bus = PrivateBus::start();
    let (host, events) = start(&bus);
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(properties("A")),
    );
    item.register(&item.unique_name()).unwrap();
    let id = events.added().id.clone();

    drop(item);
    assert_eq!(events.removed(), id);
    assert_eq!(host.handle().items(), Vec::<Arc<Item>>::new());
}

#[test]
fn the_watcher_lists_and_announces_items_read_or_not() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let signals = WatcherSignals::on(&bus);
    // An item that never answers is still registered, listed and announced.
    let item = StandIn::start(&bus, "/StatusNotifierItem", Reply::Silent);
    item.register(&item.unique_name()).unwrap();
    let id = format!("{}/StatusNotifierItem", item.unique_name());

    assert_eq!(
        signals.next(),
        ("StatusNotifierItemRegistered".into(), Some(id.clone()))
    );
    let listed: Vec<String> = watcher_property(&bus, "RegisteredStatusNotifierItems");
    assert_eq!(listed, std::slice::from_ref(&id));
    events.quiet();

    drop(item);
    assert_eq!(
        signals.next(),
        ("StatusNotifierItemUnregistered".into(), Some(id))
    );
    // The caller never heard of it, so it hears nothing of it leaving.
    events.quiet();
}

#[test]
fn registering_again_changes_nothing() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(properties("A")),
    );
    item.register(&item.unique_name()).unwrap();
    events.added();
    item.register(&item.unique_name()).unwrap();
    events.quiet();
}

#[test]
fn registering_a_service_nobody_owns_is_refused() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(HashMap::new()),
    );
    assert_eq!(
        error_name(item.register("org.example.Nobody")),
        "org.freedesktop.DBus.Error.ServiceUnknown"
    );
    assert_eq!(
        error_name(item.register(":1.9999")),
        "org.freedesktop.DBus.Error.ServiceUnknown"
    );
    events.quiet();
}
