//! The installed applications and their icons, followed for a shell
//! component on rinch: what the launcher offers and searches, and what the
//! taskbar draws its windows' buttons with.
//!
//! - [`Applications`]: every installed entry by its ID, shown in menus or
//!   not, kept up to date from the catalog's changes; the ones shown in
//!   menus ([`Applications::shown`]), and the one a window belongs to by its
//!   application id ([`Applications::for_app_id`]). Pure, and tested on its
//!   own.
//! - [`Installed`]: the applications scanned at start and watched, handed to
//!   the UI as a signal of the whole catalog at each change.
//! - [`Icons`]: the icon theme's index, built on a thread of its own for the
//!   theme the shell uses and followed as it or its files change, handed to
//!   the UI as a signal.
//! - [`path_of`]: the file an application's icon is drawn from, the generic
//!   program's when it names none or the theme lacks it; [`icon_scale`]:
//!   the scale to draw it at.

mod applications;
mod icons;
mod installed;

pub use applications::Applications;
pub use icons::{Icons, icon_scale, path_of};
pub use installed::Installed;
