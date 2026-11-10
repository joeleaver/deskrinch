//! Owning the name, as the daemon's request flags decide, and losing the bus.

use std::sync::Arc;

use deskrinch_notification_service::{
    Config, Event, ManualClock, NameRequest, NameStatus, Service, StartError,
};

use crate::common::{Client, PrivateBus, start, start_with};

fn with_name(bus: &PrivateBus, name: NameRequest) -> Config {
    Config {
        name,
        ..bus.config()
    }
}

#[test]
fn a_taken_name_is_a_typed_error() {
    let bus = PrivateBus::start();
    let (first, _events, _clock) = start(&bus);
    assert_eq!(first.initial_status(), NameStatus::Owned);
    assert!(first.handle().owns_name());
    let second = Service::start(bus.config(), Arc::new(ManualClock::new()), |_| {});
    assert!(matches!(second, Err(StartError::NameTaken)), "{second:?}");
    // Asking to replace does not help when the owner does not allow it.
    let replace = NameRequest {
        replace_existing: true,
        ..NameRequest::default()
    };
    let third = Service::start(
        with_name(&bus, replace),
        Arc::new(ManualClock::new()),
        |_| {},
    );
    assert!(matches!(third, Err(StartError::NameTaken)), "{third:?}");
}

#[test]
fn dropping_the_service_releases_the_name() {
    let bus = PrivateBus::start();
    let (first, _events, _clock) = start(&bus);
    drop(first);
    let (second, events, _clock) = start(&bus);
    assert_eq!(second.initial_status(), NameStatus::Owned);
    Client::new(&bus).simple(0, "reaches the second", "", 0);
    assert_eq!(events.next_shown().summary, "reaches the second");
}

#[test]
fn a_service_that_allows_replacement_loses_the_name_to_one_that_replaces() {
    let bus = PrivateBus::start();
    let yielding = NameRequest {
        allow_replacement: true,
        ..NameRequest::default()
    };
    let (old, old_events) = start_with(with_name(&bus, yielding), Arc::new(ManualClock::new()));
    let replace = NameRequest {
        replace_existing: true,
        ..NameRequest::default()
    };
    let (new, new_events) = start_with(with_name(&bus, replace), Arc::new(ManualClock::new()));
    assert_eq!(new.initial_status(), NameStatus::Owned);
    assert_eq!(old_events.next(), Event::NameLost);
    assert!(!old.handle().owns_name());
    Client::new(&bus).simple(0, "to the new owner", "", 0);
    assert_eq!(new_events.next_shown().summary, "to the new owner");
}

#[test]
fn a_queued_service_acquires_the_name_when_the_owner_leaves() {
    let bus = PrivateBus::start();
    let (owner, _events, _clock) = start(&bus);
    let queue = NameRequest {
        queue: true,
        ..NameRequest::default()
    };
    let (queued, events) = start_with(with_name(&bus, queue), Arc::new(ManualClock::new()));
    assert_eq!(queued.initial_status(), NameStatus::Queued);
    assert!(!queued.handle().owns_name());
    drop(owner);
    assert_eq!(events.next(), Event::NameAcquired);
    assert!(queued.handle().owns_name());
    Client::new(&bus).simple(0, "to the one that waited", "", 0);
    assert_eq!(events.next_shown().summary, "to the one that waited");
}

#[test]
fn losing_the_bus_is_reported() {
    let mut bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    bus.kill();
    assert_eq!(events.next(), Event::Disconnected);
    assert!(!service.handle().owns_name());
}
