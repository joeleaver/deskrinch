//! The other end of the launcher's interface: what `deskrinch-launcher
//! toggle`, `show` and `hide` send to the running launcher.

use deskrinch_bus::Bus;
use zbus::Connection;

use crate::service::{INTERFACE, NAME, PATH};

/// What to ask the running launcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Request {
    /// Show it if hidden, hide it if shown.
    Toggle,
    /// Show it.
    Show,
    /// Hide it.
    Hide,
}

impl Request {
    fn method(self) -> &'static str {
        match self {
            Self::Toggle => "Toggle",
            Self::Show => "Show",
            Self::Hide => "Hide",
        }
    }
}

/// What came of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Answer {
    /// The launcher did it. For a toggle, whether it is now shown.
    Done {
        /// Whether a toggle left it shown; `None` for a show or a hide.
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
