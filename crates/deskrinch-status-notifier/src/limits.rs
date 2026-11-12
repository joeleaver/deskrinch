//! Bounds on what the items, and each connection, may cost.

use std::time::Duration;

/// The bounds the watcher and the host hold every item to.
///
/// Items come from untrusted applications. A string over its bound is
/// truncated (at a character boundary); a pixmap over its bound, or whose
/// data does not match its size, is dropped; beyond the pixmap count the
/// rest are dropped. A reply over its bound, one that does not arrive in
/// time, or one that would take the images of all items over their total,
/// is refused as a whole: an item never read is never reported, and an item
/// already reported keeps what was last read. A registration over a count
/// bound is refused with a D-Bus error.
///
/// Menus are held to the same rules, per field and per structure. A label
/// over its bound is truncated, an entry's icon that is too large or does
/// not decode is dropped (as is one that would take the images over their
/// total), and shortcuts beyond their count are dropped. A layout deeper,
/// or with more entries, than its bounds allow, or a reply over its size,
/// is refused as a whole: entries are never silently left out. The
/// defaults suit a desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Registered items at once, from all connections together, read or
    /// not.
    pub max_items: usize,
    /// Registered items at once from one connection.
    pub max_items_per_connection: usize,
    /// Hosts other than this one registered with the watcher at once.
    pub max_hosts: usize,
    /// How long one read of an item's properties may take.
    pub read_timeout: Duration,
    /// The least time between the starts of two reads of one item. An item
    /// that signals faster (an animated icon, say) is read at this pace,
    /// with its latest properties.
    pub min_read_interval: Duration,
    /// Bytes of one reply to the read of an item's properties, as it
    /// arrived; the bus has already received it whole (dbus-daemon bounds a
    /// message to 128 MiB), but nothing larger is decoded.
    pub max_reply_bytes: usize,
    /// Bytes of a short string: the `Id`, the `Title`, a tooltip's title.
    pub max_text_bytes: usize,
    /// Bytes of a tooltip's description that are parsed as markup; the
    /// rest is ignored.
    pub max_description_markup_bytes: usize,
    /// Bytes of text a tooltip's description may hold once parsed.
    pub max_description_bytes: usize,
    /// Bytes of an icon name or the icon theme path; of all the directories
    /// of a menu's `IconThemePath` together.
    pub max_reference_bytes: usize,
    /// Pixmaps of one icon.
    pub max_pixmaps: usize,
    /// Width and height, in pixels, of one pixmap.
    pub max_image_side: u32,
    /// Bytes of decoded pixels across all read items, and the open menus'
    /// entries, together.
    pub max_image_bytes_total: usize,
    /// How long opening a menu (or a submenu) waits for the application's
    /// answer to `AboutToShow`, in which applications build their menus,
    /// before its layout is read anyway.
    pub about_to_show_timeout: Duration,
    /// Bytes of one reply to the read of a menu's layout, as it arrived;
    /// nothing larger is decoded.
    pub max_menu_reply_bytes: usize,
    /// Levels of submenus below a menu's own entries.
    pub max_menu_depth: usize,
    /// Entries of one menu, submenus included, hidden ones too.
    pub max_menu_entries: usize,
    /// Entries of one menu or submenu, directly.
    pub max_menu_entries_per_level: usize,
    /// Bytes of one entry's PNG icon, before decoding.
    pub max_menu_icon_bytes: usize,
    /// Shortcuts shown for one entry.
    pub max_shortcuts: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_items: 128,
            max_items_per_connection: 16,
            max_hosts: 16,
            read_timeout: Duration::from_secs(5),
            min_read_interval: Duration::from_millis(100),
            max_reply_bytes: 4 * 1024 * 1024,
            max_text_bytes: 256,
            max_description_markup_bytes: 16 * 1024,
            max_description_bytes: 4096,
            max_reference_bytes: 4096,
            max_pixmaps: 8,
            max_image_side: 512,
            max_image_bytes_total: 64 * 1024 * 1024,
            about_to_show_timeout: Duration::from_secs(1),
            max_menu_reply_bytes: 4 * 1024 * 1024,
            max_menu_depth: 8,
            max_menu_entries: 1024,
            max_menu_entries_per_level: 256,
            max_menu_icon_bytes: 256 * 1024,
            max_shortcuts: 4,
        }
    }
}
