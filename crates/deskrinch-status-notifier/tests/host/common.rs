//! A private bus for each test, the host on it, the events it reports, and
//! the watcher as another client sees it.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use deskrinch_status_notifier::{Bus, Config, Event, Host, Item, ItemId, Limits};
use futures_core::Stream as _;
use zbus::message::Type;
use zbus::wire::OwnedVariant;
use zbus::{Connection, MatchRule, Message, MessageStream};

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
    T: TryFrom<OwnedVariant>,
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
    let value: OwnedVariant = reply.body().deserialize().unwrap();
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
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.kde.StatusNotifierWatcher")
            .build()
            .unwrap();
        Self(forward(bus.client(), Some(rule), |message| {
            let name = message.header().member()?.to_string();
            Some((name, message.body().deserialize::<String>().ok()))
        }))
    }

    pub(crate) fn next(&self) -> WatcherSignal {
        self.0
            .recv_timeout(PATIENCE)
            .expect("no watcher signal arrived in time")
    }
}

/// The values of `RegisteredStatusNotifierItems` that `PropertiesChanged`
/// announces, as a bystander caching the watcher's properties sees them.
pub(crate) struct ListChanges(Receiver<Vec<String>>);

impl ListChanges {
    pub(crate) fn on(bus: &PrivateBus) -> Self {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .member("PropertiesChanged")
            .path("/StatusNotifierWatcher")
            .build()
            .unwrap();
        Self(forward(bus.client(), Some(rule), |message| {
            let (interface, mut changed, _invalidated): (
                String,
                HashMap<String, OwnedVariant>,
                Vec<String>,
            ) = message.body().deserialize().ok()?;
            let list = changed.remove("RegisteredStatusNotifierItems")?;
            (interface == "org.kde.StatusNotifierWatcher").then(|| list.try_into().unwrap())
        }))
    }

    pub(crate) fn next(&self) -> Vec<String> {
        self.0
            .recv_timeout(PATIENCE)
            .expect("no change of the item list was announced in time")
    }
}

/// The names every `GetAll` on the bus is addressed to, as a monitor
/// (`org.freedesktop.DBus.Monitoring`) sees them.
pub(crate) struct ReadDestinations(Receiver<String>);

impl ReadDestinations {
    /// Starts monitoring; reads sent after this returns are all seen.
    pub(crate) fn on(bus: &PrivateBus) -> Self {
        let connection = bus.client();
        // The monitor's own stream first, so that nothing after the call is
        // missed.
        let stream = MessageStream::from(&connection);
        zbus::block_on(connection.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(vec!["type='method_call',member='GetAll'"], 0_u32),
        ))
        .unwrap();
        Self(forward_stream(connection, stream, |message| {
            let header = message.header();
            if header.message_type() != Type::MethodCall
                || header.member().map(zbus::names::MemberName::as_str) != Some("GetAll")
            {
                return None;
            }
            header.destination().map(ToString::to_string)
        }))
    }

    /// The destinations of the `count` reads expected, and of any more seen
    /// while things settle. The monitor passes on what it sees from a
    /// thread of its own, so a read the host has finished may still be on
    /// its way here.
    pub(crate) fn seen(&self, count: usize) -> Vec<String> {
        let mut seen = Vec::with_capacity(count);
        while seen.len() < count {
            match self.0.recv_timeout(PATIENCE) {
                Ok(destination) => seen.push(destination),
                Err(_) => return seen,
            }
        }
        while let Ok(destination) = self.0.recv_timeout(QUIET) {
            seen.push(destination);
        }
        seen
    }
}

/// Forwards what `pick` makes of each message `rule` matches on
/// `connection` (every message without a rule), from a thread of its own.
fn forward<T: Send + 'static>(
    connection: Connection,
    rule: Option<MatchRule<'static>>,
    pick: impl Fn(&Message) -> Option<T> + Send + 'static,
) -> Receiver<T> {
    let stream = match rule {
        Some(rule) => {
            zbus::block_on(MessageStream::for_match_rule(rule, &connection, None)).unwrap()
        }
        None => MessageStream::from(&connection),
    };
    forward_stream(connection, stream, pick)
}

fn forward_stream<T: Send + 'static>(
    connection: Connection,
    mut stream: MessageStream,
    pick: impl Fn(&Message) -> Option<T> + Send + 'static,
) -> Receiver<T> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _connection = connection;
        zbus::block_on(async move {
            while let Some(Ok(message)) =
                std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx)).await
            {
                if let Some(value) = pick(&message)
                    && tx.send(value).is_err()
                {
                    return;
                }
            }
        });
    });
    rx
}
