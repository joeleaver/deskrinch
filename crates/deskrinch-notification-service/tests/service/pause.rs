//! Pausing and resuming expiry from the UI, on a manual clock.

use std::time::Duration;

use deskrinch_notification_service::OperationError;

use crate::common::{Client, PrivateBus, QUIET, Signal, id, start};

#[test]
fn a_paused_notification_expires_only_after_it_resumes() {
    let bus = PrivateBus::start();
    let (service, _events, clock) = start(&bus);
    let client = Client::new(&bus);
    let n = client.simple(0, "a", "", 1000);
    let handle = service.handle();
    clock.advance(Duration::from_millis(400));
    handle.pause(id(n)).unwrap();
    clock.advance(Duration::from_secs(3600));
    client.signals.none_within(QUIET);
    handle.resume(id(n)).unwrap();
    clock.advance(Duration::from_millis(599));
    client.signals.none_within(QUIET);
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
