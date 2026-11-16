//! The icon a popup is drawn with.
//!
//! - The first icon name in the specification's order of a notification's
//!   pictures: its image (the `image-path` hint, which may name an icon:
//!   `notify-send -i` sends it there), then its `app_icon`.
//! - Else the icon of the application its `desktop-entry` hint names, from
//!   the installed applications (`deskrinch-applications`), the generic
//!   program's when that application names none the theme has.
//! - Else none.
//!
//! An image the sender sends as pixels (`image-data`) or names as a file
//! (`image-path` or `app_icon`) is not drawn, and the next picture in the
//! order stands in: rinch cannot be handed pixels yet, and a file a
//! stranger names is only shown once it can be decoded under our bounds
//! and handed over the same way.

use deskrinch_applications::{Applications, path_of, path_of_name};
use deskrinch_desktop_entries::DesktopId;
use deskrinch_icons::Index;
use deskrinch_notification_service::{IconName, ImageSource, Notification};

/// What a notification asks to be drawn with, of what the popups draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wanted {
    /// An icon of the theme, by its name.
    Named(IconName),
    /// The icon of an installed application, by its desktop ID.
    Application(DesktopId),
    /// Nothing the popups draw.
    Nothing,
}

impl Wanted {
    /// What `notification` asks for.
    pub(crate) fn of(notification: &Notification) -> Self {
        Self::from_parts(
            [notification.image.as_ref(), notification.app_icon.as_ref()],
            notification.desktop_entry.as_deref(),
        )
    }

    /// What a notification with `pictures` (its image and its `app_icon`, in
    /// that order) and the `desktop_entry` hint asks for.
    fn from_parts(pictures: [Option<&ImageSource>; 2], desktop_entry: Option<&str>) -> Self {
        let named = pictures
            .into_iter()
            .flatten()
            .find_map(|picture| match picture {
                ImageSource::Icon(name) => Some(name.clone()),
                ImageSource::File(_) | ImageSource::Pixels(_) => None,
            });
        if let Some(name) = named {
            return Self::Named(name);
        }
        desktop_entry
            .and_then(|entry| DesktopId::new(format!("{entry}.desktop")))
            .map_or(Self::Nothing, Self::Application)
    }

    /// The file it is drawn from, `size` logical pixels square at `scale`,
    /// in `index` and among `applications`; `None` when there is none to
    /// draw.
    pub(crate) fn path(
        &self,
        index: &Index,
        applications: &Applications,
        size: u32,
        scale: u32,
    ) -> Option<String> {
        match self {
            Self::Named(name) => path_of_name(index, name.as_str(), size, scale),
            Self::Application(id) => {
                let entry = applications.get(id)?;
                path_of(index, entry.icon.as_ref(), size, scale)
            }
            Self::Nothing => None,
        }
    }
}

#[cfg(test)]
mod tests;
