//! The tray acting on items through the handle.

use deskrinch_status_notifier::{Activation, ItemId, OperationError, Orientation};

use crate::common::{PrivateBus, start};
use crate::stand_in::{Call, Reply, StandIn, properties};

/// A host on `bus` with one reported stand-in item.
fn host_and_item(bus: &PrivateBus) -> (deskrinch_status_notifier::Host, StandIn, ItemId) {
    let (host, events) = start(bus);
    let item = StandIn::start(
        bus,
        "/StatusNotifierItem",
        Reply::Properties(properties("A")),
    );
    item.register(&item.unique_name()).unwrap();
    let id = events.added().id.clone();
    (host, item, id)
}

#[test]
fn each_activation_reaches_the_item() {
    let bus = PrivateBus::start();
    let (host, item, id) = host_and_item(&bus);
    let handle = host.handle();
    let cases = [
        (Activation::Primary { x: 1, y: 2 }, Call::Activate(1, 2)),
        (
            Activation::Secondary { x: 3, y: 4 },
            Call::SecondaryActivate(3, 4),
        ),
        (
            Activation::ContextMenu { x: 5, y: 6 },
            Call::ContextMenu(5, 6),
        ),
        (
            Activation::Scroll {
                delta: -3,
                orientation: Orientation::Horizontal,
            },
            Call::Scroll(-3, "horizontal".into()),
        ),
        (
            Activation::Scroll {
                delta: 1,
                orientation: Orientation::Vertical,
            },
            Call::Scroll(1, "vertical".into()),
        ),
    ];
    for (activation, call) in cases {
        handle.activate(&id, activation, None).unwrap();
        assert_eq!(item.next_call(), call);
    }
    item.no_call();
}

#[test]
fn a_token_is_provided_before_the_activation() {
    let bus = PrivateBus::start();
    let (host, item, id) = host_and_item(&bus);
    host.handle()
        .activate(&id, Activation::Primary { x: 0, y: 0 }, Some("token-1"))
        .unwrap();
    assert_eq!(item.next_call(), Call::Token("token-1".into()));
    assert_eq!(item.next_call(), Call::Activate(0, 0));
}

#[test]
fn an_item_that_left_cannot_be_activated() {
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
    assert_eq!(
        host.handle()
            .activate(&id, Activation::Primary { x: 0, y: 0 }, None),
        Err(OperationError::NotFound(id))
    );
}

#[test]
fn after_the_host_stops_its_handles_report_it() {
    let bus = PrivateBus::start();
    let (host, _item, id) = host_and_item(&bus);
    let handle = host.handle();
    drop(host);
    assert_eq!(
        handle.activate(&id, Activation::Primary { x: 0, y: 0 }, None),
        Err(OperationError::Stopped)
    );
}
