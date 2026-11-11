//! The bus side: the watcher object, reading and calling items, and the
//! streams of signals the host follows.

pub(crate) mod item;
pub(crate) mod streams;
pub(crate) mod watcher;

/// The well-known name the watcher owns.
pub(crate) const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
/// Where the watcher object is served.
pub(crate) const WATCHER_PATH: &str = "/StatusNotifierWatcher";
/// The items' interface.
pub(crate) const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
/// Where an item registered by its service alone is.
pub(crate) const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";
/// The host's name, before `-<pid>-<n>`.
pub(crate) const HOST_NAME_PREFIX: &str = "org.kde.StatusNotifierHost";
