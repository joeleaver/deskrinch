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
/// bound is refused with a D-Bus error. The defaults suit a desktop.
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
    /// Bytes of an icon name or the icon theme path.
    pub max_reference_bytes: usize,
    /// Pixmaps of one icon.
    pub max_pixmaps: usize,
    /// Width and height, in pixels, of one pixmap.
    pub max_image_side: u32,
    /// Bytes of decoded pixels across all read items together.
    pub max_image_bytes_total: usize,
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
        }
    }
}
