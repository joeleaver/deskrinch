//! Reading an item's properties, and calling its methods.

use zbus::Connection;
use zbus::message::{Flags, Message};

use crate::dbus::ITEM_INTERFACE;
use crate::host::Activation;
use crate::item::ItemId;
use crate::limits::Limits;
use crate::model::{Outcome, Target};
use crate::parse;
use crate::wire::Properties;

/// Reads the properties of the item `id` at `target`, within `limits`. The
/// connection's method timeout is the read's.
pub(crate) async fn read(
    connection: Connection,
    id: ItemId,
    target: Target,
    limits: Limits,
) -> (ItemId, Outcome) {
    let reply = connection
        .call_method(
            Some(target.service.as_str()),
            target.path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(ITEM_INTERFACE,),
        )
        .await;
    let properties = match reply {
        Ok(reply) if reply.data().len() <= limits.max_reply_bytes => {
            reply.body().deserialize::<Properties>().ok()
        }
        Err(zbus::Error::MethodError(name, _, _)) if gone(name.as_str()) => {
            return (id, Outcome::Gone);
        }
        // Too large, timed out, or refused by the item.
        Ok(_) | Err(_) => None,
    };
    let outcome = properties.map_or(Outcome::Failed, |properties| {
        Outcome::Read(Box::new(parse::item(id.clone(), properties, &limits)))
    });
    (id, outcome)
}

/// Whether the bus's error for a call says its destination has no owner.
fn gone(error: &str) -> bool {
    matches!(
        error,
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
