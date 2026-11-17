//! A private bus for each test, a client of the service on it, and the
//! events and signals the test waits for.
//!
//! Nothing here touches the session bus: every connection is to the
//! `dbus-daemon` the test starts, whose configuration has no service
//! directories, so nothing can be activated on it either.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use deskrinch_notification_service::{
    Bus, Clock, Config, Event, ManualClock, NotificationId, Service,
};
pub(crate) use deskrinch_test_notify::{Client, Signal, Signals};
use deskrinch_test_wait::recv;
use zbus::Connection;

/// The test's own bus (see `deskrinch-test-dbus`), stopped when dropped.
pub(crate) struct PrivateBus(deskrinch_test_dbus::PrivateBus);

impl PrivateBus {
    pub(crate) fn start() -> Self {
        Self(deskrinch_test_dbus::PrivateBus::start().expect("a private bus (needs dbus-daemon)"))
    }

    pub(crate) fn address(&self) -> &str {
        self.0.address()
    }

    pub(crate) fn config(&self) -> Config {
        Config {
            bus: Bus::Address(self.address().to_owned()),
            ..Config::default()
        }
    }

    /// A client connection to this bus.
    pub(crate) fn client(&self) -> Connection {
        zbus::block_on(zbus::connection::Builder::address(self.address()).build()).unwrap()
    }

    /// A client of the service, on a connection of its own to this bus.
    pub(crate) fn notify_client(&self) -> Client {
        Client::on(&self.client())
    }

    /// The signals a connection of its own that sends nothing receives:
    /// only those broadcast to everyone.
    pub(crate) fn bystander(&self) -> Signals {
        Signals::on(&self.client())
    }

    /// A command for `program` in the bus's isolated session, pointed at
    /// the bus.
    pub(crate) fn command(&self, program: &str) -> std::process::Command {
        self.0.command(program)
    }

    /// Starts `command` (from [`command`](Self::command)), killed should
    /// the test process die without ending it.
    pub(crate) fn spawn(
        &self,
        command: std::process::Command,
    ) -> std::io::Result<std::process::Child> {
        self.0.spawn(command)
    }

    /// Stops the daemon now.
    pub(crate) fn kill(&mut self) {
        self.0.stop();
    }
}

/// The events a service reported, in order, but for those about the
/// sentinels of [`Time`](crate::time::Time).
pub(crate) struct Events {
    events: Receiver<Event>,
    sentinels: Mutex<HashSet<u32>>,
}

impl Events {
    pub(crate) fn next(&self) -> Event {
        loop {
            let event = recv("an event from the service", &self.events);
            let about = match &event {
                Event::Shown(n) | Event::Replaced(n) => Some(n.id.get()),
                Event::Closed { id, .. } => Some(id.get()),
                _ => None,
            };
            let sentinels = self.sentinels.lock().unwrap();
            if !about.is_some_and(|id| sentinels.contains(&id)) {
                return event;
            }
        }
    }

    /// Leaves out, from now on, the events about the notification `id`.
    pub(crate) fn leave_out(&self, id: u32) {
        self.sentinels.lock().unwrap().insert(id);
    }

    pub(crate) fn next_shown(&self) -> Arc<deskrinch_notification_service::Notification> {
        match self.next() {
            Event::Shown(n) => n,
            other => panic!("expected Shown, got {other:?}"),
        }
    }
}

/// Starts a service with `config` on `clock`, collecting its events.
pub(crate) fn start_with(config: Config, clock: Arc<dyn Clock>) -> (Service, Events) {
    let (tx, rx) = mpsc::channel();
    let service = Service::start(config, clock, move |event| {
        // The test may have stopped listening; then nobody needs the event.
        let _sent = tx.send(event);
    })
    .unwrap();
    let events = Events {
        events: rx,
        sentinels: Mutex::default(),
    };
    (service, events)
}

/// Starts a service on `bus` with a manual clock.
pub(crate) fn start(bus: &PrivateBus) -> (Service, Events, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new());
    let dyn_clock: Arc<dyn Clock> = Arc::<ManualClock>::clone(&clock);
    let (service, events) = start_with(bus.config(), dyn_clock);
    (service, events, clock)
}

pub(crate) fn id(value: u32) -> NotificationId {
    NotificationId::new(value).unwrap()
}
