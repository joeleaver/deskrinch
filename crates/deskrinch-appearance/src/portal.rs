//! The settings portal on the bus: who owns it, what it says, and the
//! signals that say it changed.
//!
//! The portal is `org.freedesktop.portal.Desktop` at
//! `/org/freedesktop/portal/desktop`; its `org.freedesktop.portal.Settings`
//! interface has `ReadOne` (version 2) and the older `Read`, whose answer
//! is wrapped in one more variant, and the signal `SettingChanged`.

use zbus::DBusError as _;
use zbus::OwnedVariant;
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream};

use crate::error::AppearanceError;
use crate::scheme::ColorScheme;

/// The portal's well-known name.
pub(crate) const PORTAL: &str = "org.freedesktop.portal.Desktop";
/// Its object.
pub(crate) const PATH: &str = "/org/freedesktop/portal/desktop";
/// The settings interface.
pub(crate) const SETTINGS: &str = "org.freedesktop.portal.Settings";
/// The appearance settings' namespace.
pub(crate) const NAMESPACE: &str = "org.freedesktop.appearance";
/// The colour scheme's key.
pub(crate) const COLOR_SCHEME: &str = "color-scheme";

const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// The bus's signals about who owns the portal's name.
pub(crate) async fn owner_changes(connection: &Connection) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(BUS)
        .interface(BUS)
        .path(BUS_PATH)
        .member("NameOwnerChanged")
        .arg(0, PORTAL)
        .build()?;
    MessageStream::for_match_rule(rule, connection, None).await
}

/// `SettingChanged` signals in the appearance namespace, from whoever sends
/// them; the following thread keeps only the portal owner's.
pub(crate) async fn setting_changes(connection: &Connection) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(SETTINGS)
        .path(PATH)
        .member("SettingChanged")
        .arg(0, NAMESPACE)
        .build()?;
    MessageStream::for_match_rule(rule, connection, None).await
}

/// The unique name that owns the portal's name, or `None`.
pub(crate) async fn owner(connection: &Connection) -> Result<Option<String>, AppearanceError> {
    let reply = connection
        .call_method(Some(BUS), BUS_PATH, Some(BUS), "GetNameOwner", &(PORTAL,))
        .await;
    match reply {
        Ok(reply) => reply
            .body()
            .deserialize::<String>()
            .map(Some)
            .map_err(AppearanceError::Call),
        Err(error) if is_error(&error, "org.freedesktop.DBus.Error.NameHasNoOwner") => Ok(None),
        Err(error) => Err(AppearanceError::Call(error)),
    }
}

/// Asks the bus to start the portal (D-Bus activation), as calling its
/// name would, and waits for the bus's answer. `Ok` once it runs; an error
/// when the bus cannot start it (no service file, or activation off).
pub(crate) async fn start(connection: &Connection) -> Result<(), zbus::Error> {
    connection
        .call_method(
            Some(BUS),
            BUS_PATH,
            Some(BUS),
            "StartServiceByName",
            &(PORTAL, 0_u32),
        )
        .await
        .map(drop)
}

/// The colour scheme the portal owned by `owner` says. A portal that does
/// not provide the key (`NotFound`) has no preference.
pub(crate) async fn color_scheme(
    connection: &Connection,
    owner: &str,
) -> Result<ColorScheme, AppearanceError> {
    match read(connection, owner, "ReadOne").await {
        Err(error) if is_error(&error, "org.freedesktop.DBus.Error.UnknownMethod") => {
            read(connection, owner, "Read").await
        }
        result => result,
    }
    .map_or_else(
        |error| {
            if is_error(&error, "org.freedesktop.portal.Error.NotFound") {
                Ok(ColorScheme::NoPreference)
            } else {
                Err(AppearanceError::Call(error))
            }
        },
        |value| ColorScheme::from_owned(&value),
    )
}

async fn read(connection: &Connection, owner: &str, method: &str) -> zbus::Result<OwnedVariant> {
    let reply = connection
        .call_method(
            Some(owner),
            PATH,
            Some(SETTINGS),
            method,
            &(NAMESPACE, COLOR_SCHEME),
        )
        .await?;
    reply.body().deserialize::<OwnedVariant>()
}

/// Whether `error` is the D-Bus error `name`: one zbus knows (the
/// `org.freedesktop.DBus.Error` family, which it parses), or another.
fn is_error(error: &zbus::Error, name: &str) -> bool {
    match error {
        zbus::Error::MethodError(found, _, _) => found.as_str() == name,
        zbus::Error::FDO(known) => known.name().as_str() == name,
        _ => false,
    }
}
