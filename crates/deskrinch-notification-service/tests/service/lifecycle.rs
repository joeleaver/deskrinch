//! Replacement, expiry, closing, dismissal and actions, through the wire.

use std::collections::HashMap;
use std::time::Duration;

use deskrinch_notification_service::{CloseReason, Event, OperationError};
use zbus::Variant;

use crate::common::{Client, PrivateBus, QUIET, Signal, id, start};
use deskrinch_test_wait::PATIENCE;

#[test]
fn replacement_keeps_the_id_and_emits_no_close() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let first = client.simple(0, "Downloading", "10%", 0);
    let other = client.simple(0, "Other", "", 0);
    let again = client.simple(first, "Downloading", "50%", 0);
    assert_eq!(again, first);
    events.next_shown();
    events.next_shown();
    match events.next() {
        Event::Replaced(n) => {
            assert_eq!((n.id.get(), n.body.plain_text().as_str()), (first, "50%"));
        }
        other => panic!("expected Replaced, got {other:?}"),
    }
    let live: Vec<_> = service
        .handle()
        .notifications()
        .iter()
        .map(|n| n.id.get())
        .collect();
    assert_eq!(live, [first, other], "the replacement kept its place");
    signals.none_within(QUIET);

    // An id that is no longer live makes a new notification with that id,
    // as the specification's return value requires.
    client.close(first).unwrap();
    assert_eq!(signals.next(), Signal::Closed(first, 3));
    let again = client.simple(first, "Downloading", "again", 0);
    assert_eq!(again, first);
    let fixed = client.simple(4242, "Fixed id", "", 0);
    assert_eq!(fixed, 4242);
}

#[test]
fn expiry_follows_the_clock_without_sleeping() {
    let bus = PrivateBus::start();
    let (service, events, clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let short = client.simple(0, "short", "", 1000);
    let default = client.simple(0, "default", "", -1);
    let never = client.simple(0, "never", "", 0);
    for _ in 0..3 {
        events.next_shown();
    }

    clock.advance(Duration::from_millis(999));
    signals.none_within(QUIET);
    clock.advance(Duration::from_millis(1));
    assert_eq!(signals.next(), Signal::Closed(short, 1));
    assert_eq!(
        events.next(),
        Event::Closed {
            id: id(short),
            reason: CloseReason::Expired
        }
    );

    // The default timeout is five seconds.
    clock.advance(Duration::from_millis(3999));
    signals.none_within(QUIET);
    clock.advance(Duration::from_millis(1));
    assert_eq!(signals.next(), Signal::Closed(default, 1));

    clock.advance(Duration::from_secs(3600));
    signals.none_within(QUIET);
    let live: Vec<_> = service
        .handle()
        .notifications()
        .iter()
        .map(|n| n.id.get())
        .collect();
    assert_eq!(live, [never]);
}

#[test]
fn replacing_restarts_the_timer() {
    let bus = PrivateBus::start();
    let (_service, _events, clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let n = client.simple(0, "a", "", 1000);
    clock.advance(Duration::from_millis(600));
    client.simple(n, "a2", "", 1000);
    clock.advance(Duration::from_millis(600));
    signals.none_within(QUIET);
    clock.advance(Duration::from_millis(400));
    assert_eq!(signals.next(), Signal::Closed(n, 1));
}

#[test]
fn close_notification_closes_once() {
    let bus = PrivateBus::start();
    let (_service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let n = client.simple(0, "a", "", 0);
    events.next_shown();
    client.close(n).unwrap();
    assert_eq!(signals.next(), Signal::Closed(n, 3));
    assert_eq!(
        events.next(),
        Event::Closed {
            id: id(n),
            reason: CloseReason::Closed
        }
    );
    for gone in [n, 0, 12345] {
        let err = client.close(gone).unwrap_err();
        let zbus::Error::MethodError(name, message, _) = &err else {
            panic!("expected a method error, got {err:?}");
        };
        assert_eq!(
            name.as_str(),
            "org.freedesktop.Notifications.Error.NotFound"
        );
        assert_eq!(
            message.as_deref().unwrap_or(""),
            "",
            "the specification's empty error"
        );
    }
    signals.none_within(QUIET);
}

#[test]
fn the_user_dismisses() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let n = client.simple(0, "a", "", 0);
    events.next_shown();
    let handle = service.handle();
    handle.dismiss(id(n)).unwrap();
    assert_eq!(signals.next(), Signal::Closed(n, 2));
    assert_eq!(handle.dismiss(id(n)), Err(OperationError::NotFound(id(n))));
}

#[test]
fn invoking_an_action_sends_the_token_then_the_action_then_closes() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let actions = ["default", "Open", "reply", "Reply"];
    let n = client
        .send(0, "a", "", &actions, &HashMap::new(), 0)
        .unwrap();
    events.next_shown();
    let handle = service.handle();
    assert_eq!(
        handle.invoke_action(id(n), "forward", None),
        Err(OperationError::UnknownAction {
            id: id(n),
            key: "forward".into()
        })
    );
    handle
        .invoke_action(id(n), "reply", Some("xdg-token-1".into()))
        .unwrap();
    assert_eq!(
        signals.next(),
        Signal::ActivationToken(n, "xdg-token-1".into())
    );
    assert_eq!(signals.next(), Signal::ActionInvoked(n, "reply".into()));
    assert_eq!(signals.next(), Signal::Closed(n, 2));
}

#[test]
fn a_resident_notification_stays_after_an_action() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let signals = &client.signals;
    let hints = HashMap::from([("resident", Variant::from(true))]);
    let n = client
        .send(0, "music", "", &["default", "Show"], &hints, 0)
        .unwrap();
    events.next_shown();
    let handle = service.handle();
    handle.invoke_action(id(n), "default", None).unwrap();
    assert_eq!(signals.next(), Signal::ActionInvoked(n, "default".into()));
    signals.none_within(QUIET);
    assert!(handle.notification(id(n)).is_some());
}

#[test]
fn the_subscriber_may_call_back_into_the_handle() {
    let bus = PrivateBus::start();
    let clock = std::sync::Arc::new(deskrinch_notification_service::ManualClock::new());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle_slot: std::sync::Arc<std::sync::OnceLock<deskrinch_notification_service::Handle>> =
        std::sync::Arc::default();
    let slot = std::sync::Arc::clone(&handle_slot);
    let service =
        deskrinch_notification_service::Service::start(bus.config(), clock, move |event| {
            // Dismiss everything as soon as it is shown, from inside the callback.
            if let (Event::Shown(n), Some(handle)) = (&event, slot.get()) {
                handle.dismiss(n.id).unwrap();
            }
            let _sent = tx.send(event);
        })
        .unwrap();
    handle_slot.set(service.handle()).unwrap();
    let n = Client::new(&bus).simple(0, "a", "", 0);
    let first = rx.recv_timeout(crate::common::PATIENCE).unwrap();
    assert!(matches!(first, Event::Shown(_)));
    let second = rx.recv_timeout(crate::common::PATIENCE).unwrap();
    assert_eq!(
        second,
        Event::Closed {
            id: id(n),
            reason: CloseReason::Dismissed
        }
    );
}
