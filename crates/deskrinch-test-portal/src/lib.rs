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
//! - [`StandIn`]: a portal on a bus that answers `org.freedesktop.appearance`
//!   `color-scheme` with a value, or `NotFound`; changes it and signals
//!   `SettingChanged`; offers `ReadOne` and `Read`, or only the older
//!   `Read`; and leaves the bus when stopped. Start it after a client to
//!   have a portal that appears late.
//! - [`ActivatingBus`]: a private bus (`deskrinch-test-dbus`) that starts a
//!   stand-in through D-Bus activation when a client asks for the portal.
//! - [`serve_from_env`]: the stand-in as a process of its own, configured by
//!   its environment: what a bus activates, and what the
//!   `deskrinch-test-portal` binary runs in an isolated session, where it
//!   can follow a file so its answer can be changed while a component runs.

mod activation;
mod serve;
mod stand_in;

pub use activation::ActivatingBus;
pub use serve::{SCHEME_FILE, SCHEME_VALUE, serve_from_env};
pub use stand_in::{Methods, StandIn, number};

/// The portal's well-known name.
pub const NAME: &str = "org.freedesktop.portal.Desktop";
/// Its object.
pub const PATH: &str = "/org/freedesktop/portal/desktop";
/// The settings interface.
pub const INTERFACE: &str = "org.freedesktop.portal.Settings";
/// The appearance settings' namespace.
pub const NAMESPACE: &str = "org.freedesktop.appearance";
/// The colour scheme's key.
pub const KEY: &str = "color-scheme";

/// Why the stand-in could not run.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The bus could not be used.
    #[error("the stand-in portal's bus failed")]
    Bus(#[from] zbus::Error),
    /// A private bus could not be started.
    #[error("cannot start a private bus")]
    PrivateBus(#[from] deskrinch_test_dbus::Error),
    /// The service file could not be written.
    #[error("cannot write the stand-in's service file")]
    ServiceFile(#[source] std::io::Error),
    /// The environment does not say where to serve or what to answer.
    #[error("{0}")]
    Environment(String),
}
