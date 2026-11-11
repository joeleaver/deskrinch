//! What the host reports about an item: its identity, text, status, icons,
//! tooltip and menu.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use deskrinch_untrusted::{IconName, Image};

/// An item's identity on the bus: its service and object path, written as
/// the watcher announces it (`:1.42/org/ayatana/NotificationItem/app`, or
/// `org.kde.StatusNotifierItem-1234-1/StatusNotifierItem`).
///
/// It stays the same for as long as the item is registered. An item that
/// leaves and registers again under the same service and path gets the same
/// identity back; a unique name (`:1.42`) is never reused by the bus.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(Arc<str>);

impl ItemId {
    pub(crate) fn new(service: &str, path: &str) -> Self {
        Self([service, path].concat().into())
    }

    /// The identity as the watcher announces it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the item says about its state (the `Status` property).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Status {
    /// Nothing needs the user; a tray may hide it.
    Passive,
    /// The item is in use. A status the item does not name, or names with a
    /// value outside the specification, reads as this.
    #[default]
    Active,
    /// The item asks for the user's attention: show its attention icon.
    NeedsAttention,
}

impl Status {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "Passive" => Self::Passive,
            "NeedsAttention" => Self::NeedsAttention,
            _ => Self::Active,
        }
    }
}

/// What kind of item it is (the `Category` property), which a tray may group
/// or order by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Category {
    /// An application's own status. A category the item does not name, or
    /// names with a value outside the specification, reads as this.
    #[default]
    ApplicationStatus,
    /// A communication application: chat, mail.
    Communications,
    /// A system service: a package manager, a disk indexer.
    SystemServices,
    /// Hardware status: a battery, the network, the sound card.
    Hardware,
}

impl Category {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "Communications" => Self::Communications,
            "SystemServices" => Self::SystemServices,
            "Hardware" => Self::Hardware,
            _ => Self::ApplicationStatus,
        }
    }
}

/// One of the item's icons, as a themed name, as pixels, or both.
///
/// The host never resolves the name: the caller looks it up in the icon
/// theme, in the item's [`Item::icon_theme_path`] first. When both are
/// present, the name is the item's first choice (Plasma prefers it too) and
/// the pixels its fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Icon {
    /// The themed icon's name.
    pub name: Option<IconName>,
    /// The same icon at one or more sizes, smallest first, each validated
    /// and decoded to RGBA.
    pub pixmaps: Vec<Arc<Image>>,
}

impl Icon {
    /// Whether the item gave neither a name nor any valid pixels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.pixmaps.is_empty()
    }

    pub(crate) fn image_bytes(&self) -> usize {
        self.pixmaps.iter().map(|p| p.rgba().len()).sum()
    }
}

/// The item's tooltip (the `ToolTip` property).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolTip {
    /// An icon to show in the tooltip.
    pub icon: Icon,
    /// The tooltip's title.
    pub title: String,
    /// The text under the title, as plain text. The item may send it with
    /// the notification markup subset (`<b>`, `<i>`, `<a>`, `<br>` and
    /// character references); the markup is parsed and dropped, references
    /// decoded and `<br>` made a line break, so it holds no tags and no
    /// undecoded entities (`deskrinch_untrusted::markup`).
    pub description: String,
}

/// A registered item whose properties were read, within bounds and in time.
///
/// Every string is cleaned of control characters and bounded, and every
/// pixmap validated, before an `Item` is made ([`Limits`](crate::Limits)).
/// An empty [`title`](Self::title) is common (Chromium sends one): the UI
/// decides what to show instead, such as the tooltip's title or
/// [`app_id`](Self::app_id).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    /// The item's identity on the bus.
    pub id: ItemId,
    /// The application's own name for the item (the `Id` property), which
    /// stays the same across runs; empty when the item gave none.
    pub app_id: String,
    /// What kind of item it is.
    pub category: Category,
    /// A name for the item; may be empty.
    pub title: String,
    /// What the item says about its state.
    pub status: Status,
    /// The icon to show normally.
    pub icon: Icon,
    /// An icon to draw over [`icon`](Self::icon), such as a badge.
    pub overlay: Icon,
    /// The icon to show while the status is [`Status::NeedsAttention`].
    pub attention: Icon,
    /// A directory of icons the item ships, to search before the theme:
    /// an absolute path, which the host never reads.
    pub icon_theme_path: Option<PathBuf>,
    /// The tooltip, when the item has one with any text or icon.
    pub tooltip: Option<ToolTip>,
    /// The item only shows its menu: activating it should open the menu
    /// (`ItemIsMenu`).
    pub item_is_menu: bool,
    /// The object path of the item's `com.canonical.dbusmenu` menu.
    pub(crate) menu: Option<String>,
}

impl Item {
    /// Whether the item exports a menu. A `Menu` of `/`, which libayatana
    /// sends when there is none, is no menu.
    #[must_use]
    pub fn has_menu(&self) -> bool {
        self.menu.is_some()
    }

    /// The decoded pixel bytes the item holds, for the host's total bound.
    pub(crate) fn image_bytes(&self) -> usize {
        self.icon.image_bytes()
            + self.overlay.image_bytes()
            + self.attention.image_bytes()
            + self.tooltip.as_ref().map_or(0, |t| t.icon.image_bytes())
    }
}
