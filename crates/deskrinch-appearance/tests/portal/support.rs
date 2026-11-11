//! The stand-in portal (`deskrinch-test-portal`) on a private bus, and the
//! events the crate reports about it.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_appearance::{Appearance, Bus, Event};
use deskrinch_test_dbus::PrivateBus;
use deskrinch_test_portal::{ActivatingBus, Methods, StandIn};
use zbus::OwnedVariant;

pub(crate) use deskrinch_test_portal::{INTERFACE, KEY, NAMESPACE, PATH, number};

/// A private bus for one test.
pub(crate) fn bus() -> PrivateBus {
    PrivateBus::start().expect("the appearance tests need dbus-daemon")
}

/// A stand-in portal on `bus` answering `value` (`None`: no key).
pub(crate) fn portal(bus: &PrivateBus, value: Option<OwnedVariant>, methods: Methods) -> StandIn {
    StandIn::start(bus.address(), value, methods).unwrap()
}

/// A bus that starts the stand-in when asked, answering `scheme`: it runs
/// this test binary's ignored `activation::stand_in`.
pub(crate) fn activating_bus(scheme: u32) -> ActivatingBus {
    let exe = std::env::current_exe().unwrap();
    ActivatingBus::start(
        scheme,
        &exe,
        &["--exact", "activation::stand_in", "--ignored", "--quiet"],
    )
    .expect("the appearance tests need dbus-daemon")
}

/// Follows the portal on `bus`, with the events in a channel.
pub(crate) fn follow(bus: &PrivateBus) -> (Appearance, Receiver<Event>) {
    let (send, events) = mpsc::channel();
    let appearance = Appearance::start(Bus::Address(bus.address().to_owned()), move |event| {
        send.send(event).ok();
    })
    .unwrap();
    (appearance, events)
}

/// The next event, within a few seconds.
pub(crate) fn next(events: &Receiver<Event>) -> Event {
    events
        .recv_timeout(Duration::from_secs(5))
        .expect("an event within 5 s")
}

/// That nothing more arrives for a moment.
pub(crate) fn quiet(events: &Receiver<Event>) {
    if let Ok(event) = events.recv_timeout(Duration::from_millis(300)) {
        panic!("unexpected {event:?}");
    }
}
