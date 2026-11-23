//! The launcher's session-bus interface from the caller's side: where it
//! is, what it offers, and the calls to it.
//!
//! Its callers are the launcher's own subcommands (`deskrinch-launcher
//! toggle`, `show`, `hide` and `forget-starts`) and the launcher's tests.
//!
//! # The interface
//!
//! The running launcher owns [`NAME`], whose object [`PATH`] has the
//! interface [`INTERFACE`]:
//!
//! - `Toggle() -> b`: shows the launcher if hidden, hides it if shown, and
//!   answers whether it is now shown;
//! - `Show()`, `Hide()`: shows or hides it; one already shown or hidden
//!   stays so;
//! - `ForgetStarts()`: deletes the record of the applications started,
//!   and returns once it is deleted;
//! - the property `Shown` (`b`, read-only): whether it is shown, or about
//!   to be, read on demand: no `PropertiesChanged` is sent for it.
//!
//! A caller [`send`]s a [`Request`] and reads `Shown` with [`shown`]; with
//! no launcher on the bus, either answers so ([`Answer::NotRunning`],
//! [`Showing::NotRunning`]) rather than failing.

use deskrinch_bus::Bus;
use zbus::{Connection, OwnedVariant};

/// The launcher's well-known name.
pub const NAME: &str = "org.deskrinch.Launcher";
/// Its object.
pub const PATH: &str = "/org/deskrinch/Launcher";
/// Its interface.
pub const INTERFACE: &str = "org.deskrinch.Launcher1";

/// The interface's property saying whether the launcher is shown.
const SHOWN: &str = "Shown";

/// A method of the interface to call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Request {
    /// `Toggle`.
    Toggle,
    /// `Show`.
    Show,
    /// `Hide`.
    Hide,
    /// `ForgetStarts`.
    ForgetStarts,
}

impl Request {
    /// The method of [`INTERFACE`] it calls.
    fn method(self) -> &'static str {
        match self {
            Self::Toggle => "Toggle",
            Self::Show => "Show",
            Self::Hide => "Hide",
            Self::ForgetStarts => "ForgetStarts",
        }
    }
}

/// What came of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Answer {
    /// The launcher did it. For a toggle, whether it is now shown.
    Done {
        /// Whether a toggle left it shown; `None` for any other request.
        shown: Option<bool>,
    },
    /// No launcher is running on the bus.
    NotRunning,
}

/// What the launcher's `Shown` property said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Showing {
    /// It is shown, or about to be.
    Shown,
    /// It is hidden.
    Hidden,
    /// No launcher is running on the bus.
    NotRunning,
}

/// Why a call could not be made.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The bus could not be reached, or the call failed.
    #[error("cannot reach the launcher over the bus")]
    Bus(#[source] zbus::Error),
}

/// Sends `request` to the launcher on `bus`.
///
/// # Errors
///
/// [`ClientError::Bus`] when the bus cannot be reached or the call fails
/// for another reason than there being no launcher.
pub fn send(bus: &Bus, request: Request) -> Result<Answer, ClientError> {
    zbus::block_on(async {
        let connection = bus.connect().await.map_err(ClientError::Bus)?;
        call(&connection, request).await
    })
}

/// Reads the launcher's `Shown` property on `bus`.
///
/// # Errors
///
/// [`ClientError::Bus`] when the bus cannot be reached, or the read fails
/// for another reason than there being no launcher.
pub fn shown(bus: &Bus) -> Result<Showing, ClientError> {
    zbus::block_on(async {
        let connection = bus.connect().await.map_err(ClientError::Bus)?;
        let reply = connection
            .call_method(
                Some(NAME),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(INTERFACE, SHOWN),
            )
            .await;
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) if not_running(&error) => return Ok(Showing::NotRunning),
            Err(error) => return Err(ClientError::Bus(error)),
        };
        let value: OwnedVariant = reply.body().deserialize().map_err(ClientError::Bus)?;
        let shown = bool::try_from(value).map_err(ClientError::Bus)?;
        Ok(if shown {
            Showing::Shown
        } else {
            Showing::Hidden
        })
    })
}

/// Sends `request` over `connection`.
async fn call(connection: &Connection, request: Request) -> Result<Answer, ClientError> {
    let reply = connection
        .call_method(Some(NAME), PATH, Some(INTERFACE), request.method(), &())
        .await;
    match reply {
        Ok(reply) if request == Request::Toggle => {
            let shown = reply
                .body()
                .deserialize::<bool>()
                .map_err(ClientError::Bus)?;
            Ok(Answer::Done { shown: Some(shown) })
        }
        Ok(_) => Ok(Answer::Done { shown: None }),
        Err(error) if not_running(&error) => Ok(Answer::NotRunning),
        Err(error) => Err(ClientError::Bus(error)),
    }
}

/// Whether `error` says no launcher owns [`NAME`].
fn not_running(error: &zbus::Error) -> bool {
    matches!(
        error,
        zbus::Error::FDO(error)
            if matches!(
                **error,
                zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
            )
    )
}
