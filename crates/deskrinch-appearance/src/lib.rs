//! The system's appearance preferences, from the settings portal: today the
//! colour scheme (dark, light, or no preference), read and followed.
//!
//! # Where it comes from
//!
//! The settings portal (`org.freedesktop.portal.Settings`, served as
//! `org.freedesktop.portal.Desktop` at `/org/freedesktop/portal/desktop`)
//! publishes the desktop's appearance in the namespace
//! `org.freedesktop.appearance`, whatever the compositor: KDE, GNOME and the
//! wlroots desktops all provide it through `xdg-desktop-portal`. This crate
//! reads its `color-scheme` key and follows the portal's `SettingChanged`
//! signal.
//!
//! The same namespace has `accent-color` and `contrast`; they are not read
//! yet.
//!
//! # What it copes with
//!
//! - A portal nothing has started yet: following asks the bus to start it
//!   (`StartServiceByName`, the D-Bus activation every portal client
//!   triggers by calling it), so following may start the portal.
//! - No portal on the bus, and none the bus can start: [`Event::NoPortal`].
//!   A portal that appears later
//!   is noticed (through the bus's `NameOwnerChanged`) and read; one that
//!   goes away is [`Event::NoPortal`] again.
//! - A portal without the key: no preference.
//! - A value the specification does not define: no preference, as the
//!   specification says ([`ColorScheme::from_portal`]). A value that is not
//!   a number is an [`Event::Error`].
//! - A portal with only the older `Read` method: it is used when `ReadOne`
//!   is unknown.
//!
//! What the user wants the shell to look like (following this, or always
//! dark or light) is the caller's decision; this crate only reports the
//! portal.
//!
//! # Structure
//!
//! [`Appearance`] runs on a thread of its own, on zbus's own runtime, and
//! calls a subscriber; no function needs an async runtime. Reading the
//! portal's values ([`ColorScheme`]) is pure.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_appearance::{Appearance, Bus, Event};
//!
//! let appearance = Appearance::start(Bus::Session, |event| match event {
//!     Event::ColorScheme(scheme) => println!("{scheme:?}"),
//!     Event::NoPortal => println!("no settings portal"),
//!     Event::Error(error) => eprintln!("{error}"),
//! })?;
//! # drop(appearance);
//! # Ok::<(), deskrinch_appearance::AppearanceError>(())
//! ```

mod error;
mod follow;
mod portal;
mod scheme;

pub use error::AppearanceError;
pub use follow::{Appearance, Bus, Event};
pub use scheme::ColorScheme;
