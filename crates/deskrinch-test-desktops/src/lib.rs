//! KWin's virtual desktops, through its own D-Bus interface
//! (`org.kde.KWin.VirtualDesktopManager` on the session's bus), for
//! deskrinch's compositor tests: the compositor's side of a test of
//! something that follows the desktops, made by KWin's own means rather
//! than by the protocol under test.
//!
//! It is for nested KWin sessions only, and refuses anything else itself:
//! every function panics, before it touches a bus, unless
//! [`deskrinch_test_client::Compositor::nested`] says the process runs in
//! a nested KWin (`cargo xtask compositor-tests kwin` or
//! `cargo xtask session kwin`). Run directly, it would lay out, switch, add
//! and remove the desktops of the real session's KWin, moving its windows.
//! A call KWin refuses panics, naming it.

use deskrinch_bus::Bus;
use deskrinch_test_client::{Compositor, NotNested};
use zbus::{OwnedVariant, Variant};

const SERVICE: &str = "org.kde.KWin";
const PATH: &str = "/VirtualDesktopManager";
const INTERFACE: &str = "org.kde.KWin.VirtualDesktopManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Panics unless `nested`, the nested compositor the process runs in, is
/// KWin, saying what a call would have done.
fn require_kwin(nested: Result<Compositor, NotNested>) {
    let found = match nested {
        Ok(Compositor::Kwin) => return,
        Ok(Compositor::Sway) => "a nested sway, which has no KWin".to_owned(),
        Ok(Compositor::Labwc) => "a nested labwc, which has no KWin".to_owned(),
        Err(why) => format!("no nested session ({why})"),
    };
    panic!(
        "KWin's virtual desktops are changed only in a nested KWin session \
         (`cargo xtask compositor-tests kwin`), and this process runs in {found}; \
         run directly, the call would lay out, switch, add and remove the real \
         session's desktops, moving its windows"
    );
}

fn connection() -> zbus::Connection {
    require_kwin(Compositor::in_session());
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
/// Outside a nested KWin session, as the crate documentation says. When KWin's answer is not a list of desktops.
#[must_use]
pub fn desktops() -> Vec<(u32, String, String)> {
    Vec::try_from(get("desktops")).unwrap_or_else(|error| panic!("KWin's desktops: {error}"))
}

/// The current desktop's name.
///
/// # Panics
///
/// Outside a nested KWin session, as the crate documentation says. When KWin's current desktop is not among its desktops.
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
/// Outside a nested KWin session, as the crate documentation says. When KWin has no such desktop, or refuses.
pub fn set_current(name: &str) {
    set("current", id_of(name).as_str().into());
}

/// Lays the desktops out in `rows` rows.
///
/// # Panics
///
/// Outside a nested KWin session, as the crate documentation says. When KWin refuses.
pub fn set_rows(rows: u32) {
    set("rows", rows.into());
}

/// Adds a desktop named `name` at the end.
///
/// # Panics
///
/// Outside a nested KWin session, as the crate documentation says. When KWin refuses, or has more desktops than a `u32` counts.
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
/// Outside a nested KWin session, as the crate documentation says. When KWin has no such desktop, or refuses.
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
/// Outside a nested KWin session, as the crate documentation says. When KWin refuses, or lists no desktop.
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

#[cfg(test)]
mod tests;
