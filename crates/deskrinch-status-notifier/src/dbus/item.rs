//! Reading an item's properties, and calling its methods.

use zbus::message::{Flags, Message};
use zbus::{Connection, DBusError as _};

use crate::dbus::ITEM_INTERFACE;
use crate::host::Activation;
use crate::limits::Limits;
use crate::model::{Outcome, Source, Target};
use crate::parse;
use crate::wire::Properties;

/// Reads the properties of the item at `source`, within `limits`, from its
/// owner's unique name: a unique name is never activated, so a read never
/// starts a service. The connection's method timeout is the read's.
pub(crate) async fn read(
    connection: Connection,
    source: Source,
    limits: Limits,
) -> (Source, Outcome) {
    let reply = connection
        .call_method(
            Some(source.owner.as_str()),
            source.path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(ITEM_INTERFACE,),
        )
        .await;
    let properties = match reply {
        Ok(reply) if reply.data().len() <= limits.max_reply_bytes => {
            reply.body().deserialize::<Properties>().ok()
        }
        Err(error) if gone(&error) => return (source, Outcome::Gone),
        // Too large, timed out, or refused by the item.
        Ok(_) | Err(_) => None,
    };
    let outcome = properties.map_or(Outcome::Failed, |properties| {
        Outcome::Read(Box::new(parse::item(
            source.id.clone(),
            properties,
            &limits,
        )))
    });
    (source, outcome)
}

/// Whether the bus's error for a call says its destination does not exist.
/// zbus hands the bus's standard errors back as `fdo::Error`s, others as
/// method errors; either way the name decides.
fn gone(error: &zbus::Error) -> bool {
    let name = match error {
        zbus::Error::FDO(error) => error.name().to_string(),
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        _ => return false,
    };
    matches!(
        name.as_str(),
        "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner"
    )
}

/// Sends `activation` to the item at `target`, preceded by
/// `ProvideXdgActivationToken` when a token is given. Nothing waits for a
/// reply, so a slow or broken item cannot hold up the caller; the bus keeps
/// the two calls in order.
pub(crate) async fn activate(
    connection: &Connection,
    target: &Target,
    activation: Activation,
    token: Option<&str>,
) -> zbus::Result<()> {
    if let Some(token) = token {
        let message = call(target, "ProvideXdgActivationToken").build(&(token,))?;
        connection.send(&message).await?;
    }
    let message = match activation {
        Activation::Primary { x, y } => call(target, "Activate").build(&(x, y))?,
        Activation::Secondary { x, y } => call(target, "SecondaryActivate").build(&(x, y))?,
        Activation::ContextMenu { x, y } => call(target, "ContextMenu").build(&(x, y))?,
        Activation::Scroll { delta, orientation } => {
            call(target, "Scroll").build(&(delta, orientation.as_str()))?
        }
    };
    connection.send(&message).await
}

/// A call of the item's `method` that expects no reply and starts nothing.
/// A path or name that is not valid is reported by the builder's `build`.
fn call<'a>(target: &'a Target, method: &'a str) -> zbus::message::Builder<'a> {
    Message::method_call(target.path.as_str(), method)
        .destination(target.service.as_str())
        .interface(ITEM_INTERFACE)
        .with_flags(Flags::NO_REPLY_EXPECTED | Flags::NO_AUTO_START)
}
