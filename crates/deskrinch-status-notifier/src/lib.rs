//! The `org.kde.StatusNotifierWatcher` service, and a host that reads the
//! tray's items from the bus.
//!
//! `StatusNotifierItem` is the cross-desktop tray protocol. Its interfaces
//! carry KDE's names because KDE wrote the specification, but applications
//! use it on every desktop through libayatana-appindicator, Chromium and
//! Electron, Qt, and ksni; it is not a dependency on KDE. This crate takes
//! its types from KDE's interface definitions, the specification's
//! reference.
//!
//! This crate has no UI. It is both roles the tray needs:
//!
//! - **The watcher.** It owns `org.kde.StatusNotifierWatcher` and serves it
//!   at `/StatusNotifierWatcher`: items register with it, other hosts too,
//!   and it lists and announces them. Every registration whose service
//!   exists is accepted and listed, as Plasma's watcher does; whether an
//!   item is fit to show is the host's concern.
//! - **The host.** It owns `org.kde.StatusNotifierHost-<pid>-<n>`, reads
//!   each registered item's properties, follows the signals that say they
//!   changed, and follows each item's service until it has no owner.
//!
//! # The model
//!
//! The model is the list of [`Item`]s that have been read, in the order they
//! registered. The subscriber hears of each change as an [`Event`], in
//! order:
//!
//! - **Added** ([`Event::Added`]): a registered item was read for the first
//!   time. Until then it is *pending*: listed by the watcher, but unknown
//!   to the caller. A read happens on registration, on each of the item's
//!   `New*` signals (each one is another chance for a pending item), and
//!   when its service moves to another connection. Signals that arrive
//!   while a read is in flight ask for one more read after it, never more.
//! - **Changed** ([`Event::Changed`]): a later read found different
//!   properties.
//! - **Removed** ([`Event::Removed`]): the item's service has no owner any
//!   more.
//!
//! The tray acts on an item through a [`Handle`]: [`Handle::activate`] sends
//! a click, the context-menu request or a scroll, with an optional
//! xdg-activation token that the tray obtained. The token goes first, with
//! `ProvideXdgActivationToken`, to every item: as in Plasma, nothing asks
//! the item whether it has that method, and an item without it ignores the
//! call, which expects no reply. This crate never fetches tokens itself.
//!
//! # Untrusted input
//!
//! Every item is untrusted ([`Limits`]). Registrations are bounded, in all
//! and per connection. A read must arrive within its timeout and its reply
//! within its size, or it is refused as a whole: a pending item stays
//! pending, costing only its entry, and a reported one keeps what was last
//! read. Reads run concurrently, so a slow item delays only itself.
//! Properties are decoded straight into their types; one of the wrong type
//! reads as absent. Strings are cleaned of control characters and bounded,
//! a tooltip's markup is parsed into plain text with no tags or entities
//! left in it, icon names checked to be names and not paths, pixmaps validated (their
//! data must match their size, within bounds) and decoded to RGBA. Calls to
//! items expect no reply and start no service.
//!
//! # What real items send
//!
//! The specification leaves room, and real senders use it:
//!
//! | | libayatana-appindicator | Chromium, Electron | ksni |
//! |---|---|---|---|
//! | Registers | an object path, `/org/ayatana/NotificationItem/<id>`, on its unique name | its unique name | `org.kde.StatusNotifierItem-<pid>-<n>`, or its unique name in a sandbox |
//! | Item path | `/org/ayatana/NotificationItem/<id>` | `/StatusNotifierItem` | `/StatusNotifierItem` |
//! | Icon | `IconName` and `IconThemePath` only, never pixels | pixels, or a name in a temporary `IconThemePath` | whatever the application gives |
//! | Title | the application's | empty | the application's |
//! | Menu | `/` when there is none (read as no menu) | `/org/chromium/DbusMenu` | `/MenuBar` |
//! | `ItemIsMenu` | not sent | false | as the application says |
//! | Activation | | `SecondaryActivate` does what `Activate` does; `Scroll` is ignored | each method calls the application |
//! | Watcher restarts | registers again | registers again | registers again when the watcher's name gets an owner |
//!
//! libayatana also sends `XAyatana*` properties, which this host does not
//! read. An empty title is common, and the UI chooses what to show
//! instead.
//!
//! # Threads
//!
//! No async runtime is needed. The host runs on threads of its own; the
//! subscriber is called on one of them, never with a lock held, so it may
//! call back into a [`Handle`]. A UI forwards each event to its own thread.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_status_notifier::{Activation, Config, Event, Host};
//!
//! let host = Host::start(Config::default(), |event| match event {
//!     Event::Added(item) | Event::Changed(item) => println!("{}: {}", item.id, item.title),
//!     Event::Removed(id) => println!("{id} left"),
//!     _ => {}
//! })?;
//! let handle = host.handle();
//! for item in handle.items() {
//!     handle.activate(&item.id, Activation::Primary { x: 0, y: 0 }, None)?;
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod dbus;
mod host;
mod item;
mod limits;
mod model;
mod parse;
mod wire;

pub use deskrinch_bus::Bus;
pub use deskrinch_untrusted::{IconName, Image};
pub use host::{Activation, Config, Event, Handle, Host, OperationError, Orientation, StartError};
pub use item::{Category, Icon, Item, ItemId, Status, ToolTip};
pub use limits::Limits;
