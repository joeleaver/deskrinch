//! A private bus for each test, a client of the service on it, and the
//! events and signals the test waits for.
//!
//! Nothing here touches the session bus: every connection is to the
//! `dbus-daemon` the test starts, whose configuration has no service
//! directories, so nothing can be activated on it either.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use deskrinch_notification_service::{
    Bus, Clock, Config, Event, ManualClock, NotificationId, Service,
};
use futures_core::Stream as _;
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream, Variant};

/// Long enough for a signal that should not come to have arrived.
pub(crate) const QUIET: Duration = Duration::from_millis(200);

/// How long a test waits for something that should happen.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

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

    /// A command for `program` in the bus's isolated session, pointed at
    /// the bus.
    pub(crate) fn command(&self, program: &str) -> std::process::Command {
        self.0.command(program)
    }

    /// Stops the daemon now.
    pub(crate) fn kill(&mut self) {
        self.0.stop();
    }
}

/// The events a service reported, in order.
pub(crate) struct Events(Receiver<Event>);

impl Events {
    pub(crate) fn next(&self) -> Event {
        self.0
            .recv_timeout(PATIENCE)
            .expect("the service reported no event in time")
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
    (service, Events(rx))
}

/// Starts a service on `bus` with a manual clock.
pub(crate) fn start(bus: &PrivateBus) -> (Service, Events, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new());
    let dyn_clock: Arc<dyn Clock> = Arc::<ManualClock>::clone(&clock);
    let (service, events) = start_with(bus.config(), dyn_clock);
    (service, events, clock)
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
pub trait Notifications {
    #[expect(clippy::too_many_arguments, reason = "the specification's method")]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

    fn get_server_information(&self) -> zbus::Result<(String, String, String, String)>;
}

/// A client of the service, and the signals its own connection receives
/// (which, with sender-only delivery, are the ones about its notifications).
pub(crate) struct Client {
    pub proxy: NotificationsProxy<'static>,
    pub signals: Signals,
}

impl Client {
    pub(crate) fn new(bus: &PrivateBus) -> Self {
        let connection = bus.client();
        let signals = Signals::on(&connection);
        let proxy = zbus::block_on(NotificationsProxy::new(&connection)).unwrap();
        Self { proxy, signals }
    }

    /// Sends a notification with no actions or hints.
    pub(crate) fn simple(&self, replaces_id: u32, summary: &str, body: &str, timeout: i32) -> u32 {
        self.send(replaces_id, summary, body, &[], &HashMap::new(), timeout)
            .unwrap()
    }

    pub(crate) fn send(
        &self,
        replaces_id: u32,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        timeout: i32,
    ) -> zbus::Result<u32> {
        zbus::block_on(self.proxy.notify(
            "Test",
            replaces_id,
            "",
            summary,
            body,
            actions,
            hints,
            timeout,
        ))
    }

    pub(crate) fn close(&self, id: u32) -> zbus::Result<()> {
        zbus::block_on(self.proxy.close_notification(id))
    }
}

/// A signal of the interface, as a client saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signal {
    Closed(u32, u32),
    ActionInvoked(u32, String),
    ActivationToken(u32, String),
}

/// The interface's signals a client receives, in order.
pub(crate) struct Signals(Receiver<Signal>);

impl Signals {
    /// Listens on a connection of its own that sends nothing: it sees only
    /// signals broadcast to everyone.
    pub(crate) fn bystander(bus: &PrivateBus) -> Self {
        Self::on(&bus.client())
    }

    /// Starts listening on `connection`; signals it receives after this
    /// returns are all seen.
    pub(crate) fn on(connection: &Connection) -> Self {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.Notifications")
            .build()
            .unwrap();
        let mut stream =
            zbus::block_on(MessageStream::for_match_rule(rule, connection, None)).unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            zbus::block_on(async move {
                while let Some(Ok(message)) =
                    std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx)).await
                {
                    let header = message.header();
                    let body = message.body();
                    let signal = match header.member().map(zbus::names::MemberName::as_str) {
                        Some("NotificationClosed") => {
                            let (id, reason) = body.deserialize().unwrap();
                            Signal::Closed(id, reason)
                        }
                        Some("ActionInvoked") => {
                            let (id, key) = body.deserialize().unwrap();
                            Signal::ActionInvoked(id, key)
                        }
                        Some("ActivationToken") => {
                            let (id, token) = body.deserialize().unwrap();
                            Signal::ActivationToken(id, token)
                        }
                        _ => continue,
                    };
                    if tx.send(signal).is_err() {
                        return;
                    }
                }
            });
        });
        Self(rx)
    }

    pub(crate) fn next(&self) -> Signal {
        self.0
            .recv_timeout(PATIENCE)
            .expect("no signal arrived in time")
    }

    pub(crate) fn none_within(&self, wait: Duration) {
        if let Ok(signal) = self.0.recv_timeout(wait) {
            panic!("unexpected signal {signal:?}");
        }
    }
}

pub(crate) fn id(value: u32) -> NotificationId {
    NotificationId::new(value).unwrap()
}
