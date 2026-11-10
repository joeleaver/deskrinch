//! Bounds on what one notification, and one sender, may cost.

/// The bounds the service holds every notification to.
///
/// Notifications come from untrusted applications, so every string, list and
/// image a sender hands over is bounded here before the model keeps it. A
/// string over its bound is truncated (at a character boundary), a list over
/// its bound loses its tail, an image over its bound is dropped (the
/// notification falls back to its next image source), and a notification over
/// a count bound is refused with a D-Bus error. The defaults suit a desktop;
/// the daemon may tighten or loosen them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Live notifications at once, from all senders together.
    pub max_live: usize,
    /// Live notifications at once from one sender (one D-Bus connection).
    pub max_live_per_sender: usize,
    /// Bytes of `app_name`.
    pub max_app_name_bytes: usize,
    /// Bytes of `summary`.
    pub max_summary_bytes: usize,
    /// Bytes of the body's markup that are parsed; the rest is ignored.
    pub max_body_markup_bytes: usize,
    /// Bytes of text the parsed body may hold.
    pub max_body_text_bytes: usize,
    /// Bytes of an icon or image reference (an icon name, a path or a
    /// `file://` URI) and of a sound file path.
    pub max_reference_bytes: usize,
    /// Actions on one notification.
    pub max_actions: usize,
    /// Bytes of one action's key.
    pub max_action_key_bytes: usize,
    /// Bytes of one action's label.
    pub max_action_label_bytes: usize,
    /// Bytes of a short string hint: `category`, `desktop-entry`,
    /// `sound-name`.
    pub max_hint_bytes: usize,
    /// Width and height, in pixels, of an image sent as pixel data.
    pub max_image_side: u32,
    /// Bytes of decoded pixel data across all live notifications together.
    pub max_image_bytes_total: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_live: 256,
            max_live_per_sender: 32,
            max_app_name_bytes: 256,
            max_summary_bytes: 1024,
            max_body_markup_bytes: 64 * 1024,
            max_body_text_bytes: 16 * 1024,
            max_reference_bytes: 4096,
            max_actions: 16,
            max_action_key_bytes: 256,
            max_action_label_bytes: 256,
            max_hint_bytes: 256,
            max_image_side: 1024,
            max_image_bytes_total: 64 * 1024 * 1024,
        }
    }
}
