//! The `org.freedesktop.Notifications` object.

use std::sync::Arc;

use zbus::message::Header;
use zbus::object_server::SignalEmitter;

use crate::model::Refused;
use crate::notification::NotificationId;
use crate::request::{Hints, Request};
use crate::service::shared::{CloseError, NotifyError, Shared};

/// The well-known name the service owns.
pub(crate) const NAME: &str = "org.freedesktop.Notifications";
/// The object path the specification puts the interface at.
pub(crate) const PATH: &str = "/org/freedesktop/Notifications";
/// The specification version implemented, as `GetServerInformation`
/// reports it.
pub(crate) const SPEC_VERSION: &str = "1.3";

/// The errors the methods reply with.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.Notifications.Error")]
pub(crate) enum Error {
    /// A failure in zbus itself.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// `CloseNotification` named a notification that is not live. The
    /// specification asks for an empty error, so it has no message.
    NotFound,
    /// The sender, or the server, holds as many notifications as it may.
    LimitsExceeded(String),
    /// With [`Ownership::OriginalSender`](crate::Ownership::OriginalSender),
    /// another sender's notification was named.
    AccessDenied(String),
    /// The service is shutting down.
    Stopped,
}

/// The served object; every call goes to the shared state.
pub(crate) struct Notifications {
    pub(crate) shared: Arc<Shared>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    /// Shows a notification, or replaces the live one `replaces_id` names.
    #[expect(
        clippy::too_many_arguments,
        reason = "the specification's method signature"
    )]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "zbus hands the header to an interface method by value"
    )]
    fn notify(
        &self,
        #[zbus(header)] header: Header<'_>,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: Hints<'_>,
        expire_timeout: i32,
    ) -> Result<u32, Error> {
        let request = Request {
            sender: header.sender().map(ToString::to_string),
            app_name,
            replaces_id,
            app_icon,
            summary,
            body,
            actions,
            hints,
            expire_timeout,
        };
        match self.shared.notify(&request) {
            Ok(id) => Ok(id.get()),
            Err(NotifyError::Refused(refused @ Refused::NotOwner)) => {
                Err(Error::AccessDenied(refused.to_string()))
            }
            Err(NotifyError::Refused(
                refused @ (Refused::TooMany(_) | Refused::TooManyFromSender(_)),
            )) => Err(Error::LimitsExceeded(refused.to_string())),
            Err(NotifyError::Stopped) => Err(Error::Stopped),
        }
    }

    /// Closes a live notification; `NotificationClosed` reports reason 3.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "zbus hands the header to an interface method by value"
    )]
    fn close_notification(&self, #[zbus(header)] header: Header<'_>, id: u32) -> Result<(), Error> {
        let id = NotificationId::new(id).ok_or(Error::NotFound)?;
        let sender = header.sender().map(zbus::names::UniqueName::as_str);
        match self.shared.close_from_sender(id, sender) {
            Ok(()) => Ok(()),
            Err(CloseError::NotFound) => Err(Error::NotFound),
            Err(CloseError::NotOwner) => Err(Error::AccessDenied(
                "the notification belongs to another sender".into(),
            )),
            Err(CloseError::Stopped) => Err(Error::Stopped),
        }
    }

    /// The optional capabilities the server supports.
    fn get_capabilities(&self) -> Vec<&'static str> {
        self.shared.capabilities()
    }

    /// The server's name, vendor and version, and the specification version.
    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (String, String, String, &'static str) {
        let server = &self.shared.server;
        (
            server.name.clone(),
            server.vendor.clone(),
            server.version.clone(),
            SPEC_VERSION,
        )
    }

    /// A notification left: 1 expired, 2 dismissed, 3 closed by the sender.
    #[zbus(signal)]
    pub(crate) async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    /// The user invoked the action `action_key` on notification `id`.
    #[zbus(signal)]
    pub(crate) async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    /// The activation token for the action about to be reported by
    /// `ActionInvoked`.
    #[zbus(signal)]
    pub(crate) async fn activation_token(
        emitter: &SignalEmitter<'_>,
        id: u32,
        activation_token: &str,
    ) -> zbus::Result<()>;
}
