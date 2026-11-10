//! Who receives the signals: the notification's current sender only, or
//! everyone.

use std::collections::HashMap;
use std::sync::Arc;

use deskrinch_notification_service::{Config, ManualClock, SignalDelivery};

use crate::common::{Client, PrivateBus, QUIET, Signal, Signals, id, start, start_with};

#[test]
fn by_default_only_the_sender_hears_about_its_notification() {
    let bus = PrivateBus::start();
    let (service, _events, _clock) = start(&bus);
    let bystander = Signals::bystander(&bus);
    let client = Client::new(&bus);
    let n = client
        .send(0, "a", "", &["default", "Open"], &HashMap::new(), 0)
        .unwrap();
    service
        .handle()
        .invoke_action(id(n), "default", Some("focus-token".into()))
        .unwrap();
    assert_eq!(
        client.signals.next(),
        Signal::ActivationToken(n, "focus-token".into())
    );
    assert_eq!(
        client.signals.next(),
        Signal::ActionInvoked(n, "default".into())
    );
    assert_eq!(client.signals.next(), Signal::Closed(n, 2));
    bystander.none_within(QUIET);
}

#[test]
fn the_signals_follow_the_sender_that_replaced_it() {
    let bus = PrivateBus::start();
    let (service, _events, _clock) = start(&bus);
    let first = Client::new(&bus);
    let second = Client::new(&bus);
    let n = first.simple(0, "a", "", 0);
    second.simple(n, "a, again", "", 0);
    service.handle().dismiss(id(n)).unwrap();
    assert_eq!(second.signals.next(), Signal::Closed(n, 2));
    first.signals.none_within(QUIET);
}

#[test]
fn broadcast_delivery_reaches_everyone() {
    let bus = PrivateBus::start();
    let config = Config {
        signal_delivery: SignalDelivery::Broadcast,
        ..bus.config()
    };
    let (service, _events) = start_with(config, Arc::new(ManualClock::new()));
    let bystander = Signals::bystander(&bus);
    let client = Client::new(&bus);
    let n = client.simple(0, "a", "", 0);
    service.handle().dismiss(id(n)).unwrap();
    assert_eq!(client.signals.next(), Signal::Closed(n, 2));
    assert_eq!(bystander.next(), Signal::Closed(n, 2));
}
