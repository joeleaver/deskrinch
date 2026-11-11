//! The stand-in portal on a bus.

use std::sync::{Arc, Mutex, PoisonError};

use zbus::{Connection, OwnedVariant, Variant};

use crate::{Error, INTERFACE, KEY, NAME, NAMESPACE, PATH};

/// Which methods the stand-in's settings interface offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Methods {
    /// A current portal: `ReadOne` (version 2) and `Read`.
    Current,
    /// An older portal: `Read` only, whose answer is wrapped in one more
    /// variant.
    ReadOnly,
}

/// What the stand-in answers for `color-scheme`: a value, or `NotFound`.
type Answer = Arc<Mutex<Option<OwnedVariant>>>;

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
    answer: Answer,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Current {
    fn read_one(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        answer(&self.answer, namespace, key)
    }

    fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        wrapped(answer(&self.answer, namespace, key)?)
    }
}

/// An older portal's settings interface.
struct ReadOnly {
    answer: Answer,
}

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl ReadOnly {
    fn read(&self, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
        wrapped(answer(&self.answer, namespace, key)?)
    }
}

fn answer(answer: &Answer, namespace: &str, key: &str) -> Result<OwnedVariant, PortalError> {
    let answer = answer.lock().unwrap_or_else(PoisonError::into_inner);
    match answer.as_ref() {
        Some(value) if namespace == NAMESPACE && key == KEY => Ok(value.try_clone()?),
        _ => Err(PortalError::NotFound(format!("{namespace} {key}"))),
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
    answer: Answer,
}

impl StandIn {
    /// Serves a portal on the bus at `address` that answers `color-scheme`
    /// with `value` (`None`: `NotFound`), offering `methods`.
    ///
    /// # Errors
    ///
    /// When the bus cannot be connected to or the name cannot be owned.
    pub fn start(
        address: &str,
        value: Option<OwnedVariant>,
        methods: Methods,
    ) -> Result<Self, Error> {
        let answer: Answer = Arc::new(Mutex::new(value));
        let builder = zbus::connection::Builder::address(address).name(NAME);
        let builder = match methods {
            Methods::Current => builder.serve_at(
                PATH,
                Current {
                    answer: Arc::clone(&answer),
                },
            ),
            Methods::ReadOnly => builder.serve_at(
                PATH,
                ReadOnly {
                    answer: Arc::clone(&answer),
                },
            ),
        };
        let connection = zbus::block_on(builder.build())?;
        Ok(Self { connection, answer })
    }

    /// Changes the answer to `value`, and signals `SettingChanged` as the
    /// portal does.
    ///
    /// # Errors
    ///
    /// When the value cannot be copied or the signal sent.
    pub fn set(&self, value: OwnedVariant) -> Result<(), Error> {
        let signal = (NAMESPACE, KEY, Variant::from(value.try_clone()?));
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            PATH,
            INTERFACE,
            "SettingChanged",
            &signal,
        ))?;
        Ok(())
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

/// A colour scheme as the portal sends it: `0` no preference, `1` dark,
/// `2` light.
#[must_use]
pub fn number(value: u32) -> OwnedVariant {
    OwnedVariant::from(value)
}
