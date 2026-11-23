//! The launcher's session-bus interface from the caller's side: what
//! `deskrinch-launcher toggle`, `show`, `hide` and `forget-starts`, a key
//! binding and a test send to the running launcher.
//!
//! The running launcher owns [`NAME`], with the object [`PATH`] and the
//! interface [`INTERFACE`]; what each method does is the launcher's to say,
//! and its service describes it (`deskrinch-launcher`'s `service` module).
//! A caller names one of them with a [`Request`] and [`send`]s it; with no
//! launcher on the bus the answer is [`Answer::NotRunning`], not an error.
//!
//! It is a crate of its own, apart from the launcher, so that a caller (a
//! key binding's command, a test) does not build the launcher's toolkit and
//! renderers for one call.

use deskrinch_bus::Bus;
use zbus::Connection;

/// The launcher's well-known name.
pub const NAME: &str = "org.deskrinch.Launcher";
/// Its object.
pub const PATH: &str = "/org/deskrinch/Launcher";
/// Its interface.
pub const INTERFACE: &str = "org.deskrinch.Launcher1";

/// What to ask the running launcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Request {
    /// `Toggle`: show it if hidden, hide it if shown.
    Toggle,
    /// `Show`.
    Show,
    /// `Hide`.
    Hide,
    /// `ForgetStarts`: delete the record of the applications started, and
    /// answer once it is deleted.
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

/// Why a request could not be made.
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
        Err(zbus::Error::FDO(error))
            if matches!(
                *error,
                zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
            ) =>
        {
            Ok(Answer::NotRunning)
        }
        Err(error) => Err(ClientError::Bus(error)),
    }
}
