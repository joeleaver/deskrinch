//! A stand-in settings portal (`org.freedesktop.portal.Settings`), for the
//! tests of everything that reads the desktop's appearance, and for checking
//! a component by hand in an isolated session.
//!
//! It is written from the portal specification, apart from the client it
//! stands in front of (`deskrinch-appearance`), which is why it is a crate
//! of its own and not a feature of that one: a test double that shared the
//! client's constants would agree with the client's mistakes. Being a
//! separate crate also keeps its server-side D-Bus code out of the library's
//! own build.
//!
//! - [`StandIn`]: a portal on a bus that answers the settings it was given
//!   (any namespace and key, a number or a string) and `NotFound` for the
//!   rest; changes one and signals `SettingChanged`; offers `ReadOne` and
//!   `Read`, or only the older `Read`; and leaves the bus when stopped.
//!   Start it after a client to have a portal that appears late.
//! - [`ActivatingBus`]: a private bus (`deskrinch-test-dbus`) that starts a
//!   stand-in through D-Bus activation when a client asks for the portal.
//! - [`serve_from_env`]: the stand-in as a process of its own, answering
//!   what a settings file says and following it: what a bus activates, and
//!   what the `deskrinch-test-portal` binary runs in an isolated session,
//!   where the file can be changed while a component runs.

mod activation;
mod file;
mod serve;
mod stand_in;

pub use activation::ActivatingBus;
pub use file::{FileError, format, parse};
pub use serve::{SETTINGS_FILE, serve_from_env};
pub use stand_in::{Methods, StandIn};

/// The portal's well-known name.
pub const NAME: &str = "org.freedesktop.portal.Desktop";
/// Its object.
pub const PATH: &str = "/org/freedesktop/portal/desktop";
/// The settings interface.
pub const INTERFACE: &str = "org.freedesktop.portal.Settings";

/// One of the portal's settings: a key in a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Setting<'a> {
    /// The namespace, such as `org.freedesktop.appearance`.
    pub namespace: &'a str,
    /// The key in it, such as `color-scheme`.
    pub key: &'a str,
}

/// The colour scheme: a number, `0` no preference, `1` dark, `2` light.
pub const COLOR_SCHEME: Setting<'static> = Setting {
    namespace: "org.freedesktop.appearance",
    key: "color-scheme",
};
/// The icon theme as KDE's portal serves it, from `kdeglobals`: a string.
pub const KDE_ICON_THEME: Setting<'static> = Setting {
    namespace: "org.kde.kdeglobals.Icons",
    key: "Theme",
};
/// The icon theme as the GNOME and GTK portals serve it, from the
/// `org.gnome.desktop.interface` schema: a string.
pub const GNOME_ICON_THEME: Setting<'static> = Setting {
    namespace: "org.gnome.desktop.interface",
    key: "icon-theme",
};

/// A setting's value: the two kinds the appearance settings use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A `u32`.
    Number(u32),
    /// A string.
    Text(String),
}

impl Value {
    /// [`Value::Text`] of `text`.
    #[must_use]
    pub fn text(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<Value> for zbus::OwnedVariant {
    fn from(value: Value) -> Self {
        match value {
            Value::Number(number) => Self::from(number),
            Value::Text(text) => Self::from(zbus::Str::from(text)),
        }
    }
}

/// Why the stand-in could not run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The bus could not be used.
    #[error("the stand-in portal's bus failed")]
    Bus(#[from] zbus::Error),
    /// A private bus could not be started.
    #[error("cannot start a private bus")]
    PrivateBus(#[from] deskrinch_test_dbus::Error),
    /// The settings file could not be written.
    #[error("cannot write the stand-in's settings or service file")]
    Write(#[source] std::io::Error),
    /// The settings file could not be read.
    #[error("cannot read the stand-in's settings file")]
    Read(#[source] std::io::Error),
    /// The settings file says something that is not a setting.
    #[error("the stand-in's settings file is wrong")]
    File(#[from] FileError),
    /// The environment does not say where to serve or what to answer.
    #[error("{0}")]
    Environment(String),
}
