//! Items that send the wrong types, too much, nothing, or too many
//! registrations.

use std::sync::Arc;
use std::time::Duration;

use deskrinch_status_notifier::{Image, Item, Limits};

use crate::common::{PrivateBus, error_name, start, start_with};
use crate::stand_in::{Reply, StandIn, properties, v};

#[test]
fn properties_of_the_wrong_type_read_as_absent() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let mut sent = properties("A");
    sent.insert("Title".into(), v(7_u32));
    sent.insert("IconPixmap".into(), v(vec!["not", "pixels"]));
    sent.insert("Status".into(), v(true));
    let item = StandIn::start(&bus, "/StatusNotifierItem", Reply::Properties(sent));
    item.register(&item.unique_name()).unwrap();

    let added = events.added();
    assert_eq!(added.title, "");
    assert_eq!(added.icon.pixmaps, Vec::<Arc<Image>>::new());
    assert_eq!(added.app_id, "stand-in");
}

#[test]
fn pixmaps_that_do_not_match_their_size_are_dropped() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let mut sent = properties("A");
    let pixmaps: Vec<(i32, i32, Vec<u8>)> = vec![
        (2, 2, vec![0; 15]),
        (1, 1, vec![0xff, 1, 2, 3]),
        (-1, 1, vec![]),
    ];
    sent.insert("IconPixmap".into(), v(pixmaps));
    let item = StandIn::start(&bus, "/StatusNotifierItem", Reply::Properties(sent));
    item.register(&item.unique_name()).unwrap();

    let added = events.added();
    assert_eq!(added.icon.pixmaps.len(), 1);
    assert_eq!(added.icon.pixmaps[0].rgba(), &[1, 2, 3, 0xff]);
}

#[test]
fn an_oversized_reply_keeps_the_item_pending_until_a_later_read_fits() {
    let bus = PrivateBus::start();
    let (host, events) = start_with(bus.config(Limits {
        max_reply_bytes: 64 * 1024,
        ..Limits::default()
    }));
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(properties(&"x".repeat(128 * 1024))),
    );
    item.register(&item.unique_name()).unwrap();
    events.quiet();
    assert_eq!(host.handle().items(), Vec::<Arc<Item>>::new());

    // The next signal is another chance.
    item.set(Reply::Properties(properties("A")));
    item.signal("NewTitle");
    assert_eq!(events.added().title, "A");
}

#[test]
fn an_item_that_never_answers_holds_up_no_other() {
    let bus = PrivateBus::start();
    // Long enough that a host reading one item at a time would miss the
    // test's patience.
    let (_host, events) = start_with(bus.config(Limits {
        read_timeout: Duration::from_secs(60),
        ..Limits::default()
    }));
    let silent = StandIn::start(&bus, "/StatusNotifierItem", Reply::Silent);
    silent.register(&silent.unique_name()).unwrap();
    let item = StandIn::start(
        &bus,
        "/StatusNotifierItem",
        Reply::Properties(properties("A")),
    );
    item.register(&item.unique_name()).unwrap();
    assert_eq!(events.added().title, "A");
    // The silent one stays pending.
    events.quiet();
}

#[test]
fn an_item_whose_read_times_out_stays_pending() {
    let bus = PrivateBus::start();
    let (host, events) = start_with(bus.config(Limits {
        read_timeout: Duration::from_millis(100),
        ..Limits::default()
    }));
    let silent = StandIn::start(&bus, "/StatusNotifierItem", Reply::Silent);
    silent.register(&silent.unique_name()).unwrap();
    events.quiet();
    assert_eq!(host.handle().items(), Vec::<Arc<Item>>::new());
    silent.set(Reply::Properties(properties("A")));
    silent.signal("NewTitle");
    assert_eq!(events.added().title, "A");
}

#[test]
fn a_connection_may_register_only_so_many_items() {
    let bus = PrivateBus::start();
    let (_host, _events) = start_with(bus.config(Limits {
        max_items_per_connection: 2,
        ..Limits::default()
    }));
    let item = StandIn::start(&bus, "/a", Reply::Silent);
    item.register("/a").unwrap();
    item.register("/b").unwrap();
    assert_eq!(
        error_name(item.register("/c")),
        "org.freedesktop.DBus.Error.LimitsExceeded"
    );
}

#[test]
fn registrations_that_name_neither_a_service_nor_a_path_are_refused() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let item = StandIn::start(&bus, "/StatusNotifierItem", Reply::Silent);
    for bad in ["not a name!", "/bad//path", ""] {
        assert_eq!(
            error_name(item.register(bad)),
            "org.freedesktop.DBus.Error.InvalidArgs",
            "{bad:?}"
        );
    }
    events.quiet();
}
