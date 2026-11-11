//! A private bus for each test, the host on it, the events it reports, and
//! the watcher as another client sees it.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use deskrinch_status_notifier::{Bus, Config, Event, Host, Item, ItemId, Limits};
use futures_core::Stream as _;
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream};

/// Long enough for an event that should not come to have arrived.
pub(crate) const QUIET: Duration = Duration::from_millis(300);

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

    /// A host configuration for this bus with `limits`.
    pub(crate) fn config(&self, limits: Limits) -> Config {
        Config {
            bus: Bus::Address(self.address().to_owned()),
            limits,
        }
    }

    /// A client connection to this bus.
    pub(crate) fn client(&self) -> Connection {
        zbus::block_on(zbus::connection::Builder::address(self.address()).build()).unwrap()
    }

    /// A command for `program` in the bus's isolated session, pointed at
    /// the bus.
    pub(crate) fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
        self.0.command(program)
    }

    /// Stops the daemon now.
    pub(crate) fn kill(&mut self) {
        self.0.stop();
    }
}

/// The events a host reported, in order.
pub(crate) struct Events(Receiver<Event>);

impl Events {
    pub(crate) fn next(&self) -> Event {
        self.0
            .recv_timeout(PATIENCE)
            .expect("the host reported no event in time")
    }

    pub(crate) fn added(&self) -> Arc<Item> {
        match self.next() {
            Event::Added(item) => item,
            other => panic!("expected Added, got {other:?}"),
        }
    }

    pub(crate) fn changed(&self) -> Arc<Item> {
        match self.next() {
            Event::Changed(item) => item,
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    pub(crate) fn removed(&self) -> ItemId {
        match self.next() {
            Event::Removed(id) => id,
            other => panic!("expected Removed, got {other:?}"),
        }
    }

    pub(crate) fn quiet(&self) {
        if let Ok(event) = self.0.recv_timeout(QUIET) {
            panic!("unexpected event {event:?}");
        }
    }
}

/// Starts a host with `config`, collecting its events.
pub(crate) fn start_with(config: Config) -> (Host, Events) {
    let (tx, rx) = mpsc::channel();
    let host = Host::start(config, move |event| {
        // The test may have stopped listening; then nobody needs the event.
        let _sent = tx.send(event);
    })
    .unwrap();
    (host, Events(rx))
}

/// Starts a host on `bus` with the default limits.
pub(crate) fn start(bus: &PrivateBus) -> (Host, Events) {
    start_with(bus.config(Limits::default()))
}

/// Calls the watcher's `method` with `arg` from `connection`.
pub(crate) fn call_watcher(connection: &Connection, method: &str, arg: &str) -> zbus::Result<()> {
    zbus::block_on(connection.call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.kde.StatusNotifierWatcher"),
        method,
        &(arg,),
    ))
    .map(drop)
}

/// The watcher's property `name`, read by another client.
pub(crate) fn watcher_property<T>(bus: &PrivateBus, name: &str) -> T
where
    T: TryFrom<zbus::wire::OwnedVariant>,
    T::Error: std::fmt::Debug,
{
    let reply = zbus::block_on(bus.client().call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.kde.StatusNotifierWatcher", name),
    ))
    .unwrap();
    let value: zbus::wire::OwnedVariant = reply.body().deserialize().unwrap();
    T::try_from(value).unwrap()
}

/// The error name of a refused call.
pub(crate) fn error_name(result: zbus::Result<()>) -> String {
    use zbus::DBusError as _;
    match result {
        Err(zbus::Error::MethodError(name, _, _)) => name.to_string(),
        // zbus turns the standard errors into `fdo::Error` on arrival.
        Err(zbus::Error::FDO(error)) => error.name().to_string(),
        other => panic!("expected a D-Bus error, got {other:?}"),
    }
}

/// A watcher signal, as a bystander saw it: its name and its string
/// argument, if any.
pub(crate) type WatcherSignal = (String, Option<String>);

/// The watcher's signals a bystander receives, in order.
pub(crate) struct WatcherSignals(Receiver<WatcherSignal>);

impl WatcherSignals {
    /// Listens on a connection of its own; signals sent after this returns
    /// are all seen.
    pub(crate) fn on(bus: &PrivateBus) -> Self {
        let connection = bus.client();
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.kde.StatusNotifierWatcher")
            .build()
            .unwrap();
        let mut stream =
            zbus::block_on(MessageStream::for_match_rule(rule, &connection, None)).unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _connection = connection;
            zbus::block_on(async move {
                while let Some(Ok(message)) =
                    std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx)).await
                {
                    let header = message.header();
                    let name = header.member().unwrap().to_string();
                    let arg = message.body().deserialize::<String>().ok();
                    if tx.send((name, arg)).is_err() {
                        return;
                    }
                }
            });
        });
        Self(rx)
    }

    pub(crate) fn next(&self) -> WatcherSignal {
        self.0
            .recv_timeout(PATIENCE)
            .expect("no watcher signal arrived in time")
    }
}
