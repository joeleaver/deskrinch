//! A stand-in settings portal on a private bus, and the events the crate
//! reports about it.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deskrinch_appearance::{Appearance, Bus, Event};
use deskrinch_test_dbus::PrivateBus;
use zbus::{Connection, OwnedVariant, Variant};

pub(crate) const PORTAL: &str = "org.freedesktop.portal.Desktop";
pub(crate) const PATH: &str = "/org/freedesktop/portal/desktop";
pub(crate) const SETTINGS: &str = "org.freedesktop.portal.Settings";
pub(crate) const NAMESPACE: &str = "org.freedesktop.appearance";
pub(crate) const KEY: &str = "color-scheme";

/// What the stand-in answers for `color-scheme`: a value, or `NotFound`.
type Value = Arc<Mutex<Option<OwnedVariant>>>;

/// The portal's errors.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum PortalError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotFound(String),
}

/// The settings interface of a current portal (`ReadOne`, and `Read`).
struct Settings {
    value: Value,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Settings {
    fn read_one(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        answer(&self.value, namespace, key)
    }

    fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        wrapped(answer(&self.value, namespace, key)?)
    }
}

/// The settings interface of an older portal: `Read` only.
struct OldSettings {
    value: Value,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl OldSettings {
    fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        wrapped(answer(&self.value, namespace, key)?)
    }
}

fn answer(value: &Value, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
    let value = value.lock().unwrap();
    match value.as_ref() {
        Some(value) if namespace == NAMESPACE && key == KEY => Ok(value.try_clone()?),
        _ => Err(PortalError::NotFound(format!("{namespace} {key}"))),
    }
}

/// `Read`'s answer: the value in one more variant.
fn wrapped(value: OwnedVariant) -> Result<OwnedVariant, PortalError> {
    let inner = Variant::from(value);
    Ok(OwnedVariant::try_from(Variant::Variant(Box::new(inner)))?)
}

/// A stand-in portal: it owns the portal's name while it lives.
pub(crate) struct StandIn {
    connection: Connection,
    value: Value,
}

impl StandIn {
    /// A portal answering `value` (`None`: `NotFound`), with `ReadOne`
    /// unless `old`.
    pub(crate) fn start(bus: &PrivateBus, value: Option<OwnedVariant>, old: bool) -> Self {
        let value: Value = Arc::new(Mutex::new(value));
        let builder = zbus::connection::Builder::address(bus.address()).name(PORTAL);
        let builder = if old {
            builder.serve_at(
                PATH,
                OldSettings {
                    value: Arc::clone(&value),
                },
            )
        } else {
            builder.serve_at(
                PATH,
                Settings {
                    value: Arc::clone(&value),
                },
            )
        };
        let connection = zbus::block_on(builder.build()).unwrap();
        Self { connection, value }
    }

    /// Changes the value, and says so as the portal does.
    pub(crate) fn set(&self, value: OwnedVariant) {
        let signal = (NAMESPACE, KEY, Variant::from(value.try_clone().unwrap()));
        *self.value.lock().unwrap() = Some(value);
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            PATH,
            SETTINGS,
            "SettingChanged",
            &signal,
        ))
        .unwrap();
    }

    /// Leaves the bus, as a portal that exits does.
    pub(crate) fn stop(self) {
        zbus::block_on(self.connection.close()).unwrap();
    }
}

/// A number as the portal sends it.
pub(crate) fn number(value: u32) -> OwnedVariant {
    OwnedVariant::from(value)
}

/// Follows the portal on `bus`, with the events in a channel.
pub(crate) fn follow(bus: &PrivateBus) -> (Appearance, Receiver<Event>) {
    let (send, events) = mpsc::channel();
    let appearance = Appearance::start(Bus::Address(bus.address().to_owned()), move |event| {
        send.send(event).ok();
    })
    .unwrap();
    (appearance, events)
}

/// The next event, within a few seconds.
pub(crate) fn next(events: &Receiver<Event>) -> Event {
    events
        .recv_timeout(Duration::from_secs(5))
        .expect("an event within 5 s")
}

/// That nothing more arrives for a moment.
pub(crate) fn quiet(events: &Receiver<Event>) {
    if let Ok(event) = events.recv_timeout(Duration::from_millis(300)) {
        panic!("unexpected {event:?}");
    }
}

/// A private bus for one test.
pub(crate) fn bus() -> PrivateBus {
    PrivateBus::start().expect("the appearance tests need dbus-daemon")
}
