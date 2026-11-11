//! Reads: to whom they go, which items a signal reads, and how often.

use std::thread;
use std::time::Duration;

use deskrinch_status_notifier::Limits;

use crate::common::{PrivateBus, ReadDestinations, start, start_with};
use crate::stand_in::{Reply, StandIn, properties};

const NAME: &str = "org.kde.StatusNotifierItem-1-1";
const PATH: &str = "/StatusNotifierItem";

#[test]
fn reads_go_to_the_owner_s_unique_name_never_to_a_well_known_one() {
    let bus = PrivateBus::start();
    let reads = ReadDestinations::on(&bus);
    let (_host, events) = start(&bus);
    let item = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
    item.own(NAME);
    item.register(NAME).unwrap();
    events.added();
    item.set(Reply::Properties(properties("B")));
    item.signal("NewTitle");
    events.changed();

    // A unique name is never activated, so no read can start a service.
    assert_eq!(reads.seen(), [item.unique_name(), item.unique_name()]);
}

#[test]
fn a_well_known_name_moving_to_another_connection_is_read_from_it() {
    let bus = PrivateBus::start();
    let reads = ReadDestinations::on(&bus);
    let (host, events) = start(&bus);
    let first = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
    first.own_replaceably(NAME);
    first.register(NAME).unwrap();
    let id = events.added().id.clone();

    let second = StandIn::start(&bus, PATH, Reply::Properties(properties("B")));
    second.take_over(NAME);
    let changed = events.changed();
    assert_eq!(
        (changed.id.clone(), changed.title.as_str()),
        (id.clone(), "B")
    );
    // Its signals are followed from the new owner.
    second.set(Reply::Properties(properties("C")));
    second.signal("NewTitle");
    assert_eq!(events.changed().title, "C");
    assert_eq!(
        reads.seen(),
        [
            first.unique_name(),
            second.unique_name(),
            second.unique_name()
        ]
    );
    // The first connection leaving does not take the item with it.
    drop(first);
    events.quiet();
    assert_eq!(host.handle().items().len(), 1);
}

#[test]
fn a_read_waiting_out_its_interval_goes_to_whoever_owns_the_item_when_it_starts() {
    let bus = PrivateBus::start();
    let reads = ReadDestinations::on(&bus);
    // Long enough that the move below happens while the read waits, even
    // on a loaded machine.
    let (host, events) = start_with(bus.config(Limits {
        min_read_interval: Duration::from_secs(3),
        ..Limits::default()
    }));
    let first = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
    first.own_replaceably(NAME);
    first.register(NAME).unwrap();
    let id = events.added().id.clone();

    // A signal asks for a read, which must wait out the interval...
    first.set(Reply::Properties(properties("still A")));
    first.signal("NewTitle");
    // ...during which the name moves to another connection.
    let second = StandIn::start(&bus, PATH, Reply::Properties(properties("B")));
    second.take_over(NAME);

    // The waiting read goes to the new owner, and the caller is told what
    // it holds; the old owner, still connected, is not read again.
    let changed = events.changed();
    assert_eq!((changed.id.clone(), changed.title.as_str()), (id, "B"));
    assert_eq!(reads.seen(), [first.unique_name(), second.unique_name()]);
    assert_eq!((first.reads(), second.reads()), (1, 1));
    events.quiet();
    assert_eq!(host.handle().items()[0].title, "B");
}

#[test]
fn an_item_that_leaves_while_it_is_read_is_removed() {
    let bus = PrivateBus::start();
    let (host, events) = start(&bus);
    let item = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
    item.own(NAME);
    item.register(NAME).unwrap();
    let id = events.added().id.clone();
    // A read in flight when the connection goes ends as the bus's "no such
    // name", and the name losing its owner says the same: either way the
    // item goes, once.
    item.signal("NewTitle");
    drop(item);
    assert_eq!(events.removed(), id);
    events.quiet();
    assert_eq!(host.handle().items().len(), 0);
}

#[test]
fn one_signal_reads_every_item_registered_at_its_object() {
    let bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    let item = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
    item.own(NAME);
    item.register(NAME).unwrap();
    item.register(&item.unique_name()).unwrap();
    let mut ids = [events.added().id.clone(), events.added().id.clone()];
    ids.sort();

    item.set(Reply::Properties(properties("B")));
    item.signal("NewTitle");
    let mut changed = [events.changed().id.clone(), events.changed().id.clone()];
    changed.sort();
    assert_eq!(changed, ids);
}

#[test]
fn an_item_signalling_in_a_loop_is_read_at_the_interval() {
    let bus = PrivateBus::start();
    let interval = Duration::from_millis(200);
    let (_host, events) = start_with(bus.config(Limits {
        min_read_interval: interval,
        ..Limits::default()
    }));
    let item = StandIn::start(&bus, PATH, Reply::Properties(properties("A")));
    item.register(&item.unique_name()).unwrap();
    events.added();
    assert_eq!(item.reads(), 1);

    // An animated icon: a burst of signals, the last with new properties.
    item.set(Reply::Properties(properties("B")));
    for _ in 0..50 {
        item.signal("NewIcon");
    }
    // The latest properties are read, and reported once.
    assert_eq!(events.changed().title, "B");
    thread::sleep(interval * 3);
    events.quiet();
    // One read for the burst, and at most one more asked for while it ran:
    // nowhere near one a signal.
    let reads = item.reads();
    assert!((2..=3).contains(&reads), "{reads} reads");
}
