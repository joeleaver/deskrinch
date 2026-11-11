//! The watcher's name, the hosts, and the end of the connection.

use deskrinch_status_notifier::{Event, Host, Limits, StartError};

use crate::common::{PrivateBus, WatcherSignals, call_watcher, start, watcher_property};

#[test]
fn a_second_watcher_finds_the_name_taken() {
    let bus = PrivateBus::start();
    let (_first, _events) = start(&bus);
    let second = Host::start(bus.config(Limits::default()), |_| {});
    assert!(matches!(second, Err(StartError::NameTaken)), "{second:?}");
}

#[test]
fn the_watcher_reports_a_host_and_plasma_s_protocol_version() {
    let bus = PrivateBus::start();
    let (_host, _events) = start(&bus);
    let registered: bool = watcher_property(&bus, "IsStatusNotifierHostRegistered");
    assert!(registered);
    let version: i32 = watcher_property(&bus, "ProtocolVersion");
    assert_eq!(version, 0);
}

#[test]
fn another_host_is_announced_until_its_name_has_no_owner() {
    let bus = PrivateBus::start();
    let (_host, _events) = start(&bus);
    let signals = WatcherSignals::on(&bus);
    let other = bus.client();
    zbus::block_on(other.request_name("org.example.Host")).unwrap();
    call_watcher(&other, "RegisterStatusNotifierHost", "org.example.Host").unwrap();
    assert_eq!(
        signals.next(),
        ("StatusNotifierHostRegistered".into(), None)
    );

    drop(other);
    assert_eq!(
        signals.next(),
        ("StatusNotifierHostUnregistered".into(), None)
    );
}

#[test]
fn the_bus_going_away_is_reported() {
    let mut bus = PrivateBus::start();
    let (_host, events) = start(&bus);
    bus.kill();
    assert_eq!(events.next(), Event::Disconnected);
}
