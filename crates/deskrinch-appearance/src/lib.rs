//! The system's appearance preferences, from the settings portal: the
//! colour scheme (dark, light, or no preference) and the icon theme, read
//! and followed.
//!
//! # Where it comes from
//!
//! The settings portal (`org.freedesktop.portal.Settings`, served as
//! `org.freedesktop.portal.Desktop` at `/org/freedesktop/portal/desktop`)
//! publishes settings by namespace and key. This crate reads three, and
//! follows the portal's `SettingChanged` signal for them:
//!
//! - `org.freedesktop.appearance` `color-scheme`: the standard namespace,
//!   which KDE, GNOME and the wlroots desktops all provide through
//!   `xdg-desktop-portal`. Its `accent-color` and `contrast` are not read
//!   yet.
//! - `org.kde.kdeglobals.Icons` `Theme` and `org.gnome.desktop.interface`
//!   `icon-theme`: the icon theme. The portal specification has no standard
//!   key for it, so it is read under the two keys portals do serve it
//!   under: KDE's portal serves `kdeglobals` in the `org.kde.kdeglobals.*`
//!   namespaces, and the GNOME and GTK portals serve the
//!   `org.gnome.desktop.interface` schema. These are the portal's key
//!   names, not a dependency on either desktop: nothing here asks which
//!   desktop is running, and either key may come from any portal.
//!   [`IconThemes`] reports both; which one wins is the caller's decision.
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
//! - A portal without a key: that setting has its default (no preference,
//!   no icon theme); so has an empty icon theme name.
//! - A colour scheme the specification does not define: no preference, as
//!   the specification says ([`ColorScheme::from_portal`]).
//! - A value of the wrong type: an [`Event::Error`], and the setting stays
//!   as it was (its default, when following starts).
//! - A portal with only the older `Read` method: it is used when `ReadOne`
//!   is unknown.
//!
//! What the user wants the shell to look like (following this, or always
//! dark or light, or another icon theme) is the caller's decision; this
//! crate only reports the portal.
//!
//! # Structure
//!
//! [`Appearance`] runs on a thread of its own, on zbus's own runtime, and
//! calls a subscriber; no function needs an async runtime.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_appearance::{Appearance, Bus, Event};
//!
//! let appearance = Appearance::start(Bus::Session, |event| match event {
//!     Event::Preferences(preferences) => println!("{preferences:?}"),
//!     Event::NoPortal => println!("no settings portal"),
//!     Event::Error(error) => eprintln!("{error}"),
//! })?;
//! # drop(appearance);
//! # Ok::<(), deskrinch_appearance::AppearanceError>(())
//! ```

mod error;
mod follow;
mod portal;
mod preferences;
mod scheme;

pub use deskrinch_bus::Bus;
pub use error::AppearanceError;
pub use follow::{Appearance, Event};
pub use preferences::{IconThemes, Preferences};
pub use scheme::ColorScheme;
