//! Items registering, changing and leaving, as the host reports them and as
//! the watcher announces them.

use std::collections::HashMap;
use std::sync::Arc;

use deskrinch_status_notifier::{Item, Limits, Status};

use crate::common::{
    ListChanges, PrivateBus, STALL, WatcherSignals, error_name, start, start_with,
    watcher_property, within,
};
use crate::flood::{self, Flood, NAME_FLOOD, SIGNAL_FLOOD};
use crate::stand_in::{Reply, StandIn, properties, v};

const NAME: &str = "org.kde.StatusNotifierItem-1-1";

#[test]
fn the_host_starts_in_a_flood_of_name_changes() {
    // Guards: starting awaits no reply while the stream of owner changes
    // goes unread. That stream takes every name change on the bus from the
    // first moment, and zbus stops reading the socket while a stream's
    // queue is full; a flood of name changes (names change in bursts at
    // login, when a panel starts) fills it at once, so a reply awaited
    // beside it unread is never read. A stall is turned into a failure by
    // the watchdog. A run in which the host started before the flood had
    // filled the queue passes without having met the case.
    within(STALL, || {
        let bus = PrivateBus::start();
        let flood = Flood::start(&bus, NAME_FLOOD, flood::name_change);
        flood.wait_for(500);
        let (host, events) = start_with(bus.config(Limits::default()));
        drop(flood);
        // And it works: an item registers and is read.
        let item = StandIn::start(
            &bus,
            "/StatusNotifierItem",
            Reply::Properties(properties("A")),
        );
        item.register(&item.unique_name()).unwrap();
        assert_eq!(events.added().title, "A");
        assert_eq!(host.handle().items().len(), 1);
    });
}

#[test]
fn an_item_registers_in_a_flood_of_item_signals() {
    // Guards: the watcher's method handlers may await the bus (the
    // registering name's owner) because they hold no stream, and the
    // tracker keeps draining every stream the host holds while they wait.
    // A flood of item signals fills the items' stream's queue many times
    // over during the registration; were that stream left unread, the
    // handler's reply would not be read either, and the application's
    // registration would fail at the call's timeout (applications do not
    // try again). A stall is turned into a failure by the watchdog. A run
    // in which the flood was drained before the registration passes
    // without having met the case.
    within(STALL, || {
        let bus = PrivateBus::start();
        let (host, events) = start(&bus);
        let flood = Flood::start(&bus, SIGNAL_FLOOD, flood::item_signal);
        flood.wait_for(500);
        let item = StandIn::start(
            &bus,
            "/StatusNotifierItem",
            Reply::Properties(properties("A")),
        );
        item.register(&item.unique_name()).unwrap();
        assert_eq!(events.added().title, "A");
        drop(flood);
        assert_eq!(host.handle().items().len(), 1);
    });
}

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
fn the_item_list_announces_each_change_to_clients_that_cache_it() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let changes = ListChanges::on(&bus);
    let first = StandIn::start(&bus, "/StatusNotifierItem", Reply::Silent);
    first.register(&first.unique_name()).unwrap();
    let first_id = format!("{}/StatusNotifierItem", first.unique_name());
    assert_eq!(changes.next(), std::slice::from_ref(&first_id));

    let second = StandIn::start(&bus, "/StatusNotifierItem", Reply::Silent);
    second.register(&second.unique_name()).unwrap();
    let second_id = format!("{}/StatusNotifierItem", second.unique_name());
    assert_eq!(changes.next(), [first_id.clone(), second_id.clone()]);

    drop(first);
    assert_eq!(changes.next(), [second_id]);
    drop(second);
    assert_eq!(changes.next(), Vec::<String>::new());
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
