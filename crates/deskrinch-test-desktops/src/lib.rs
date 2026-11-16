//! KWin's virtual desktops, through its own D-Bus interface
//! (`org.kde.KWin.VirtualDesktopManager` on the session's bus), for
//! deskrinch's compositor tests: the compositor's side of a test of
//! something that follows the desktops, made by KWin's own means rather
//! than by the protocol under test.
//!
//! Every call goes to the session bus of the nested session the test runs
//! in, never the real one: the tests that use this refuse to run outside
//! one. A call KWin refuses panics, naming it.

use deskrinch_bus::Bus;
use zbus::{OwnedVariant, Variant};

const SERVICE: &str = "org.kde.KWin";
const PATH: &str = "/VirtualDesktopManager";
const INTERFACE: &str = "org.kde.KWin.VirtualDesktopManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

fn connection() -> zbus::Connection {
    zbus::block_on(Bus::Session.connect()).unwrap_or_else(|error| {
        panic!("the session bus: {error}");
    })
}

fn get(property: &str) -> OwnedVariant {
    let reply = zbus::block_on(connection().call_method(
        Some(SERVICE),
        PATH,
        Some(PROPERTIES),
        "Get",
        &(INTERFACE, property),
    ))
    .unwrap_or_else(|error| panic!("KWin's {property}: {error}"));
    reply
        .body()
        .deserialize::<OwnedVariant>()
        .unwrap_or_else(|error| panic!("KWin's {property}: {error}"))
}

fn set(property: &str, value: Variant<'_>) {
    zbus::block_on(connection().call_method(
        Some(SERVICE),
        PATH,
        Some(PROPERTIES),
        "Set",
        &(INTERFACE, property, value),
    ))
    .unwrap_or_else(|error| panic!("setting KWin's {property}: {error}"));
}

/// The id of the desktop named `name`.
fn id_of(name: &str) -> String {
    desktops()
        .into_iter()
        .find(|(_, _, listed)| listed == name)
        .map_or_else(|| panic!("KWin has no desktop {name}"), |(_, id, _)| id)
}

/// Each desktop's (position, id, name).
///
/// # Panics
///
/// When KWin's answer is not a list of desktops.
#[must_use]
pub fn desktops() -> Vec<(u32, String, String)> {
    Vec::try_from(get("desktops")).unwrap_or_else(|error| panic!("KWin's desktops: {error}"))
}

/// The current desktop's name.
///
/// # Panics
///
/// When KWin's current desktop is not among its desktops.
#[must_use]
pub fn current_name() -> String {
    let current =
        String::try_from(get("current")).unwrap_or_else(|error| panic!("KWin's current: {error}"));
    desktops()
        .into_iter()
        .find(|(_, id, _)| *id == current)
        .map_or_else(
            || panic!("KWin's current desktop {current} is not listed"),
            |(_, _, name)| name,
        )
}

/// Makes the desktop named `name` current, as KWin's own interface does.
///
/// # Panics
///
/// When KWin has no such desktop, or refuses.
pub fn set_current(name: &str) {
    set("current", id_of(name).as_str().into());
}

/// Lays the desktops out in `rows` rows.
///
/// # Panics
///
/// When KWin refuses.
pub fn set_rows(rows: u32) {
    set("rows", rows.into());
}

/// Adds a desktop named `name` at the end.
///
/// # Panics
///
/// When KWin refuses, or has more desktops than a `u32` counts.
pub fn create(name: &str) {
    let count = u32::try_from(desktops().len())
        .unwrap_or_else(|_| panic!("more desktops than a u32 counts"));
    zbus::block_on(connection().call_method(
        Some(SERVICE),
        PATH,
        Some(INTERFACE),
        "createDesktop",
        &(count, name),
    ))
    .unwrap_or_else(|error| panic!("KWin's createDesktop: {error}"));
}

/// Removes the desktop named `name`.
///
/// # Panics
///
/// When KWin has no such desktop, or refuses.
pub fn remove(name: &str) {
    let id = id_of(name);
    zbus::block_on(connection().call_method(
        Some(SERVICE),
        PATH,
        Some(INTERFACE),
        "removeDesktop",
        &(id,),
    ))
    .unwrap_or_else(|error| panic!("KWin's removeDesktop: {error}"));
}

/// At least `count` desktops, the first current, in one row. Returns the
/// first desktop's name.
///
/// # Panics
///
/// When KWin refuses, or lists no desktop.
#[must_use]
pub fn set_up(count: usize) -> String {
    while desktops().len() < count {
        create(&format!("Desktop {}", desktops().len() + 1));
    }
    set_rows(1);
    let first = desktops()
        .into_iter()
        .min_by_key(|(position, _, _)| *position)
        .map_or_else(|| panic!("KWin lists no desktop"), |(_, _, name)| name);
    set_current(&first);
    first
}
