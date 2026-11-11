//! The stand-in portal on a bus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use zbus::{Connection, OwnedVariant, Variant};

use crate::{Error, INTERFACE, NAME, PATH, Setting, Value};

/// Which methods the stand-in's settings interface offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Methods {
    /// A current portal: `ReadOne` (version 2) and `Read`.
    Current,
    /// An older portal: `Read` only, whose answer is wrapped in one more
    /// variant.
    ReadOnly,
}

/// What the stand-in answers, by namespace and key; anything else is
/// `NotFound`.
type Answers = Arc<Mutex<HashMap<(String, String), Value>>>;

/// The portal's errors.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
enum PortalError {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The setting does not exist.
    NotFound(String),
}

/// A current portal's settings interface.
struct Current {
    answers: Answers,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Current {
    fn read_one(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        answer(&self.answers, namespace, key)
    }

    fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        wrapped(answer(&self.answers, namespace, key)?)
    }
}

/// An older portal's settings interface.
struct ReadOnly {
    answers: Answers,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl ReadOnly {
    fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        wrapped(answer(&self.answers, namespace, key)?)
    }
}

fn answer(answers: &Answers, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
    let answers = answers.lock().unwrap_or_else(PoisonError::into_inner);
    match answers.get(&(namespace.to_owned(), key.to_owned())) {
        Some(value) => Ok(value.clone().into()),
        None => Err(PortalError::NotFound(format!("{namespace} {key}"))),
    }
}

/// `Read`'s answer: the value in one more variant.
fn wrapped(value: OwnedVariant) -> Result<OwnedVariant, PortalError> {
    let inner = Variant::from(value);
    Ok(OwnedVariant::try_from(Variant::Variant(Box::new(inner)))?)
}

/// A stand-in portal: it owns the portal's name on its bus while it lives.
#[derive(Debug)]
pub struct StandIn {
    connection: Connection,
    answers: Answers,
}

impl StandIn {
    /// Serves a portal on the bus at `address` that answers `settings`, and
    /// `NotFound` for any other, offering `methods`.
    ///
    /// # Errors
    ///
    /// When the bus cannot be connected to or the name cannot be owned.
    pub fn start<'a>(
        address: &str,
        settings: impl IntoIterator<Item = (Setting<'a>, Value)>,
        methods: Methods,
    ) -> Result<Self, Error> {
        let answers: HashMap<_, _> = settings
            .into_iter()
            .map(|(setting, value)| (owned(setting), value))
            .collect();
        let answers: Answers = Arc::new(Mutex::new(answers));
        let builder = zbus::connection::Builder::address(address).name(NAME);
        let builder = match methods {
            Methods::Current => builder.serve_at(
                PATH,
                Current {
                    answers: Arc::clone(&answers),
                },
            ),
            Methods::ReadOnly => builder.serve_at(
                PATH,
                ReadOnly {
                    answers: Arc::clone(&answers),
                },
            ),
        };
        let connection = zbus::block_on(builder.build())?;
        Ok(Self {
            connection,
            answers,
        })
    }

    /// Changes `setting` to `value` (adding it if it was not there), and
    /// signals `SettingChanged` as the portal does.
    ///
    /// # Errors
    ///
    /// When the signal cannot be sent.
    pub fn set(&self, setting: Setting<'_>, value: Value) -> Result<(), Error> {
        let signal = (
            setting.namespace,
            setting.key,
            Variant::from(OwnedVariant::from(value.clone())),
        );
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(owned(setting), value);
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            PATH,
            INTERFACE,
            "SettingChanged",
            &signal,
        ))?;
        Ok(())
    }

    /// Stops answering `setting`: it is `NotFound` from now on. The portal
    /// specification has no signal for a setting that goes away, so none
    /// is sent.
    pub(crate) fn remove(&self, setting: Setting<'_>) {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&owned(setting));
    }

    /// Leaves the bus, as a portal that exits does.
    ///
    /// # Errors
    ///
    /// When the connection cannot be closed cleanly.
    pub fn stop(self) -> Result<(), Error> {
        zbus::block_on(self.connection.close())?;
        Ok(())
    }
}

fn owned(setting: Setting<'_>) -> (String, String) {
    (setting.namespace.to_owned(), setting.key.to_owned())
}
