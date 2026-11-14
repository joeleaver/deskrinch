//! The stand-in portal (`deskrinch-test-portal`) on a private bus, and the
//! events the crate reports about it.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_appearance::{Appearance, Bus, Event, Preferences};
use deskrinch_test_dbus::PrivateBus;
use deskrinch_test_portal::{ActivatingBus, Methods, Setting, StandIn, Value};
use deskrinch_test_wait::recv;

/// A private bus for one test.
pub(crate) fn bus() -> PrivateBus {
    PrivateBus::start().expect("the appearance tests need dbus-daemon")
}

/// A stand-in portal on `bus` answering `settings`, and `NotFound` for the
/// rest.
pub(crate) fn portal(
    bus: &PrivateBus,
    settings: &[(Setting<'static>, Value)],
    methods: Methods,
) -> StandIn {
    StandIn::start(bus.address(), settings.iter().cloned(), methods).unwrap()
}

/// A bus that starts the stand-in when asked, answering `settings`: it runs
/// this test binary's ignored `activation::stand_in`.
pub(crate) fn activating_bus(settings: &[(Setting<'static>, Value)]) -> ActivatingBus {
    let exe = std::env::current_exe().unwrap();
    ActivatingBus::start(
        settings,
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

/// The next event.
#[track_caller]
pub(crate) fn next(events: &Receiver<Event>) -> Event {
    recv("an event", events)
}

/// The next event, which must be the preferences.
pub(crate) fn preferences(events: &Receiver<Event>) -> Preferences {
    match next(events) {
        Event::Preferences(preferences) => preferences,
        other => panic!("expected the preferences, got {other:?}"),
    }
}

/// That nothing more arrives for a moment.
pub(crate) fn quiet(events: &Receiver<Event>) {
    if let Ok(event) = events.recv_timeout(Duration::from_millis(300)) {
        panic!("unexpected {event:?}");
    }
}
