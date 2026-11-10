//! An orderly stop, and losing the name, close every live notification with
//! reason 4 so that no sender is left waiting.

use std::sync::Arc;

use deskrinch_notification_service::{
    CloseReason, Config, Event, ManualClock, NameRequest, Service,
};

use crate::common::{Client, PrivateBus, Signal, id, start, start_with};
use crate::interop::{finish, notify_send};

#[test]
fn stopping_closes_every_live_notification_paused_or_not() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let a = client.simple(0, "a", "", 1000);
    let b = client.simple(0, "b", "", 0);
    events.next_shown();
    events.next_shown();
    service.handle().pause(id(a)).unwrap();
    drop(service);
    assert_eq!(client.signals.next(), Signal::Closed(a, 4));
    assert_eq!(client.signals.next(), Signal::Closed(b, 4));
    for n in [a, b] {
        assert_eq!(
            events.next(),
            Event::Closed {
                id: id(n),
                reason: CloseReason::Undefined
            }
        );
    }
}

#[test]
fn stopping_releases_notify_send_waiting_on_its_notification() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let child = notify_send(&bus, &["--wait", "Waiting for the end"]);
    events.next_shown();
    drop(service);
    finish(child);
}

#[test]
fn losing_the_name_closes_every_live_notification() {
    let bus = PrivateBus::start();
    let yielding = NameRequest {
        allow_replacement: true,
        ..NameRequest::default()
    };
    let config = Config {
        name: yielding,
        ..bus.config()
    };
    let (_old, old_events) = start_with(config, Arc::new(ManualClock::new()));
    let client = Client::new(&bus);
    let n = client.simple(0, "a", "", 0);
    old_events.next_shown();
    let replace = Config {
        name: NameRequest {
            replace_existing: true,
            ..NameRequest::default()
        },
        ..bus.config()
    };
    let _new = Service::start(replace, Arc::new(ManualClock::new()), |_| {}).unwrap();
    assert_eq!(
        old_events.next(),
        Event::Closed {
            id: id(n),
            reason: CloseReason::Undefined
        }
    );
    assert_eq!(old_events.next(), Event::NameLost);
    assert_eq!(client.signals.next(), Signal::Closed(n, 4));
}
