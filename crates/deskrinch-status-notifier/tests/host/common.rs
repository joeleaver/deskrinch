//! A private bus for each test, the host on it, the events it reports, and
//! the watcher as another client sees it.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use deskrinch_status_notifier::{
    Bus, Config, Event, Host, Item, ItemId, Limits, Menu, MenuFailure,
};
use deskrinch_test_wait::PATIENCE;
use futures_core::Stream as _;
use zbus::message::Type;
use zbus::wire::OwnedVariant;
use zbus::{Connection, MatchRule, Message, MessageStream};

/// Long enough for an event that should not come to have arrived.
pub(crate) const QUIET: Duration = Duration::from_millis(300);

/// How long a test that a stalled host would hang ([`within`]) may take:
/// several waits of [`PATIENCE`] in a row.
pub(crate) const STALL: Duration = Duration::from_secs(60);

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

    pub(crate) fn menu(&self) -> Arc<Menu> {
        match self.next() {
            Event::Menu(menu) => menu,
            other => panic!("expected Menu, got {other:?}"),
        }
    }

    pub(crate) fn menu_failed(&self) -> (ItemId, MenuFailure) {
        match self.next() {
            Event::MenuFailed { item, reason } => (item, reason),
            other => panic!("expected MenuFailed, got {other:?}"),
        }
    }

    pub(crate) fn quiet(&self) {
        self.quiet_for(QUIET);
    }

    /// That no event arrives for `time`.
    pub(crate) fn quiet_for(&self, time: Duration) {
        if let Ok(event) = self.0.recv_timeout(time) {
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

/// Runs `test` on a thread of its own and fails unless it ends within
/// `limit`: a host that stalls (as one that awaits the bus while a stream
/// it holds fills can, for good) fails the test instead of hanging the
/// run. The stalled thread is left behind.
pub(crate) fn within(limit: Duration, test: impl FnOnce() + Send + 'static) {
    let (done, ended) = mpsc::channel();
    let thread = thread::spawn(move || {
        test();
        let _sent = done.send(());
    });
    match ended.recv_timeout(limit) {
        Ok(()) => thread.join().unwrap(),
        // It panicked: its own message says why.
        Err(mpsc::RecvTimeoutError::Disconnected) => match thread.join() {
            Ok(()) => {}
            Err(panic) => std::panic::resume_unwind(panic),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("the test stalled for {limit:?}"),
    }
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

/// A monitor of `bus` (`org.freedesktop.DBus.Monitoring`) for the method
/// calls `rules` match, forwarding what `pick` makes of each; every call
/// sent after this returns is seen.
fn monitor<T: Send + 'static>(
    bus: &PrivateBus,
    rules: &[&str],
    pick: impl Fn(&Message) -> Option<T> + Send + 'static,
) -> Receiver<T> {
    let connection = bus.client();
    // The monitor's own stream first, so that nothing after the call is
    // missed.
    let stream = MessageStream::from(&connection);
    zbus::block_on(connection.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus.Monitoring"),
        "BecomeMonitor",
        &(rules, 0_u32),
    ))
    .unwrap();
    forward_stream(connection, stream, move |message| {
        (message.header().message_type() == Type::MethodCall)
            .then(|| pick(message))
            .flatten()
    })
}

fn member_is(message: &Message, member: &str) -> bool {
    message
        .header()
        .member()
        .map(zbus::names::MemberName::as_str)
        == Some(member)
}

/// The match rules for the menus' interface held on the bus, as a monitor
/// sees connections ask for them and give them up (`AddMatch`,
/// `RemoveMatch`); only the host asks for those.
pub(crate) struct MenuMatches {
    calls: Receiver<(bool, String)>,
    live: std::cell::RefCell<Vec<String>>,
    added: std::cell::Cell<usize>,
}

impl MenuMatches {
    /// Starts monitoring, before the host starts, so that every rule it
    /// asks for is seen.
    pub(crate) fn on(bus: &PrivateBus) -> Self {
        let rules = [
            "type='method_call',interface='org.freedesktop.DBus',member='AddMatch'",
            "type='method_call',interface='org.freedesktop.DBus',member='RemoveMatch'",
        ];
        let calls = monitor(bus, &rules, |message| {
            let added = member_is(message, "AddMatch");
            let rule: String = message.body().deserialize().ok()?;
            rule.contains("com.canonical.dbusmenu")
                .then_some((added, rule))
        });
        Self {
            calls,
            live: std::cell::RefCell::new(Vec::new()),
            added: std::cell::Cell::new(0),
        }
    }

    /// The rules held once `settled` says they are as expected (the host
    /// changes them on its own thread), with any further change made while
    /// things settle.
    pub(crate) fn live(&self, settled: impl Fn(&[String]) -> bool) -> Vec<String> {
        let mut live = self.live.borrow_mut();
        let apply = |live: &mut Vec<String>, (added, rule): (bool, String)| {
            if added {
                self.added.set(self.added.get() + 1);
                live.push(rule);
            } else if let Some(at) = live.iter().position(|r| *r == rule) {
                live.remove(at);
            }
        };
        let deadline = std::time::Instant::now() + PATIENCE;
        while !settled(&live) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.calls.recv_timeout(left) {
                Ok(call) => apply(&mut live, call),
                Err(_) => break,
            }
        }
        while let Ok(call) = self.calls.recv_timeout(QUIET) {
            apply(&mut live, call);
        }
        live.clone()
    }

    /// How many times a menu's match has been asked for, as far as
    /// [`live`](Self::live) has seen.
    pub(crate) fn added(&self) -> usize {
        self.added.get()
    }
}

impl ReadDestinations {
    /// Starts monitoring; reads sent after this returns are all seen.
    pub(crate) fn on(bus: &PrivateBus) -> Self {
        let rules = ["type='method_call',member='GetAll'"];
        Self(monitor(bus, &rules, |message| {
            if !member_is(message, "GetAll") {
                return None;
            }
            message.header().destination().map(ToString::to_string)
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
