//! Pausing and resuming expiry from the UI, on a manual clock.

use std::time::Duration;

use crate::common::{Client, PrivateBus, Signal, id, start};
use crate::time::Time;
use deskrinch_notification_service::OperationError;

#[test]
fn a_paused_notification_expires_only_after_it_resumes() {
    let bus = PrivateBus::start();
    let (service, events, clock) = start(&bus);
    let client = Client::new(&bus);
    let time = Time {
        client: &client,
        clock: &clock,
        handle: service.handle(),
        events: &events,
    };
    let n = client.simple(0, "a", "", 1000);
    let handle = service.handle();
    clock.advance(Duration::from_millis(400));
    handle.pause(id(n)).unwrap();
    time.advance_quietly(Duration::from_secs(3600));
    handle.resume(id(n)).unwrap();
    time.advance_quietly(Duration::from_millis(599));
    clock.advance(Duration::from_millis(1));
    assert_eq!(client.signals.next(), Signal::Closed(n, 1));
    assert_eq!(handle.pause(id(n)), Err(OperationError::NotFound(id(n))));
}

#[test]
fn a_paused_notification_still_closes_when_dismissed() {
    let bus = PrivateBus::start();
    let (service, _events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let n = client.simple(0, "a", "", 1000);
    let handle = service.handle();
    handle.pause(id(n)).unwrap();
    handle.dismiss(id(n)).unwrap();
    assert_eq!(client.signals.next(), Signal::Closed(n, 2));
}
