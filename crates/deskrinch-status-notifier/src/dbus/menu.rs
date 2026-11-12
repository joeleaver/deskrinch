//! Reading an item's menu (`com.canonical.dbusmenu`), and telling it what
//! the user did.

use std::io::ErrorKind;
use std::time::Duration;

use deskrinch_timer::Timer;
use zbus::message::{Flags, Message};
use zbus::{Connection, DBusError as _};

use crate::dbus::item::gone;
use crate::dbus::{ITEM_INTERFACE, MENU_INTERFACE};
use crate::item::ItemId;
use crate::limits::Limits;
use crate::menu::MenuFailure;
use crate::model::{MenuOutcome, MenuSource, MenuTarget};
use crate::parse;
use crate::wire::layout::{self, Bounds, RawMenuProperties, Refused};

/// The root entry's id, which every menu has.
pub(crate) const ROOT: i32 = 0;

/// Reads the layout of the menu at `source`, within `limits`, from its
/// owner's unique name; the menu's own properties with it. The first read
/// of an opening says `AboutToShow` first and waits for the answer, in
/// which applications build their menus, for no longer than its own
/// timeout. The layout and the properties must each answer within
/// [`Limits::read_timeout`].
pub(crate) async fn read(
    connection: Connection,
    timer: Timer,
    source: MenuSource,
    limits: Limits,
) -> (MenuSource, MenuOutcome) {
    let outcome = read_menu(&connection, &timer, &source, &limits).await;
    (source, outcome)
}

async fn read_menu(
    connection: &Connection,
    timer: &Timer,
    source: &MenuSource,
    limits: &Limits,
) -> MenuOutcome {
    if source.about_to_show {
        let answer = about_to_show(
            connection,
            timer,
            &source.owner,
            &source.path,
            ROOT,
            limits.about_to_show_timeout,
        )
        .await;
        // Any answer will do, an error included (not every menu has the
        // method), unless the application has gone.
        if let Err(Failed::Gone) = answer {
            return MenuOutcome::Gone;
        }
    }
    let body = (ROOT, -1_i32, Vec::<&str>::new());
    let call = connection.call_method(
        Some(source.owner.as_str()),
        source.path.as_str(),
        Some(MENU_INTERFACE),
        "GetLayout",
        &body,
    );
    let reply = match timer.timeout(limits.read_timeout, call).await {
        Ok(Ok(reply)) if reply.data().len() <= limits.max_menu_reply_bytes => reply,
        Ok(Ok(_)) => return MenuOutcome::Failed(MenuFailure::TooLarge),
        Ok(Err(error)) => {
            return match failed(&error) {
                Failed::Gone => MenuOutcome::Gone,
                Failed::Because(reason) => MenuOutcome::Failed(reason),
            };
        }
        Err(_) => return MenuOutcome::Failed(MenuFailure::TimedOut),
    };
    let body = reply.body();
    let bounds = Bounds {
        depth: limits.max_menu_depth,
        entries: limits.max_menu_entries,
        per_level: limits.max_menu_entries_per_level,
    };
    let root = match layout::layout(body.data(), body.signature(), bounds) {
        Ok(root) => root,
        Err(Refused::OverBound) => return MenuOutcome::Failed(MenuFailure::OverBound),
        Err(Refused::Malformed) => return MenuOutcome::Failed(MenuFailure::Malformed),
    };
    let properties = properties(connection, timer, source, limits).await;
    let id: ItemId = source.id.clone();
    MenuOutcome::Read(Box::new(parse::menu::menu(id, root, properties, limits)))
}

/// The menu's own properties; none when they cannot be read, which leaves
/// the menu as it is.
async fn properties(
    connection: &Connection,
    timer: &Timer,
    source: &MenuSource,
    limits: &Limits,
) -> RawMenuProperties {
    let body = (MENU_INTERFACE,);
    let call = connection.call_method(
        Some(source.owner.as_str()),
        source.path.as_str(),
        Some("org.freedesktop.DBus.Properties"),
        "GetAll",
        &body,
    );
    match timer.timeout(limits.read_timeout, call).await {
        Ok(Ok(reply)) if reply.data().len() <= limits.max_reply_bytes => {
            reply.body().deserialize().unwrap_or_default()
        }
        Ok(_) | Err(_) => RawMenuProperties::default(),
    }
}

/// Why a call to a menu failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failed {
    /// The connection does not exist.
    Gone,
    Because(MenuFailure),
}

fn failed(error: &zbus::Error) -> Failed {
    if gone(error) {
        return Failed::Gone;
    }
    let name = match error {
        zbus::Error::InputOutput(io) if io.kind() == ErrorKind::TimedOut => {
            return Failed::Because(MenuFailure::TimedOut);
        }
        zbus::Error::FDO(error) => error.name().to_string(),
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        _ => return Failed::Because(MenuFailure::Malformed),
    };
    Failed::Because(if name == "org.freedesktop.DBus.Error.NoReply" {
        MenuFailure::TimedOut
    } else {
        MenuFailure::Refused
    })
}

/// Says `AboutToShow` for the entry `id` of the menu at `path` of
/// connection `owner`, and waits for no longer than `timeout`: whether the
/// application says the menu needs reading again.
pub(crate) async fn about_to_show(
    connection: &Connection,
    timer: &Timer,
    owner: &str,
    path: &str,
    id: i32,
    timeout: Duration,
) -> Result<bool, Failed> {
    let body = (id,);
    let call = connection.call_method(
        Some(owner),
        path,
        Some(MENU_INTERFACE),
        "AboutToShow",
        &body,
    );
    match timer.timeout(timeout, call).await {
        Ok(Ok(reply)) => reply
            .body()
            .deserialize::<bool>()
            .map_err(|_| Failed::Because(MenuFailure::Malformed)),
        Ok(Err(error)) => Err(failed(&error)),
        Err(_) => Err(Failed::Because(MenuFailure::TimedOut)),
    }
}

/// Sends the event `event` about the entry `id` to the menu at `target`,
/// preceded by `ProvideXdgActivationToken` to the item when a token is
/// given. Nothing waits for a reply, as with the item's activations; the
/// bus keeps the two calls in order.
pub(crate) async fn event(
    connection: &Connection,
    target: &MenuTarget,
    id: i32,
    event: &str,
    token: Option<&str>,
) -> zbus::Result<()> {
    let flags = Flags::NO_REPLY_EXPECTED | Flags::NO_AUTO_START;
    if let Some(token) = token {
        let message = Message::method_call(target.item_path.as_str(), "ProvideXdgActivationToken")
            .destination(target.service.as_str())
            .interface(ITEM_INTERFACE)
            .with_flags(flags)
            .build(&(token,))?;
        connection.send(&message).await?;
    }
    // The data is an int32 0 and the timestamp 0, as Plasma and
    // libdbusmenu send them: Wayland has no X server time to give.
    let message = Message::method_call(target.path.as_str(), "Event")
        .destination(target.service.as_str())
        .interface(MENU_INTERFACE)
        .with_flags(flags)
        .build(&(id, event, zbus::wire::Variant::from(0_i32), 0_u32))?;
    connection.send(&message).await
}
