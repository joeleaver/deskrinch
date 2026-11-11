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
//!   and it lists and announces them (`RegisteredStatusNotifierItems`
//!   sends `PropertiesChanged` at each change, for clients that cache it).
//!   Every registration whose service exists is accepted and listed, as
//!   Plasma's watcher does, including one made from another connection
//!   (xembedsniproxy registers that way); whether an item is fit to show is
//!   the host's concern.
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
//!   when its service moves to another connection. A signal reads every
//!   item registered at the object it came from (one connection may
//!   register an object under both its unique name and a well-known one).
//!   Signals that arrive while a read is in flight ask for one more read
//!   after it, never more, and two reads of one item start no closer
//!   together than [`Limits::min_read_interval`]: an item that signals in a
//!   loop (an animated icon) is read at that pace, with its latest
//!   properties.
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
//! `activate` never waits on the bus: the host's own thread sends the
//! calls, in order, so a UI thread may call it.
//!
//! # Menus
//!
//! An item's menu (`com.canonical.dbusmenu`) is read only while the caller
//! has it open. [`Handle::open_menu`] tells the application (`Event`
//! `opened`), asks it to get the menu ready (`AboutToShow`, in which
//! applications build their menus) and waits for its answer, but no longer
//! than [`Limits::about_to_show_timeout`], then reads the whole layout.
//! The subscriber hears of it as [`Event::Menu`], or [`Event::MenuFailed`]
//! with the reason. While it is open, each `LayoutUpdated` or
//! `ItemsPropertiesUpdated` asks for the whole layout to be read again,
//! paced and coalesced as an item's reads are, so a storm of them costs one
//! read an interval; the menu is also read again when its item moves to
//! another connection, and from its new object when the item's `Menu`
//! moves (an item that no longer has one fails the menu,
//! [`MenuFailure::NoMenu`]). [`Handle::close_menu`] tells the application
//! and releases what was read.
//!
//! An entry is named by an [`EntryId`], which names the entry the caller
//! was given, not only the application's number for it: some applications
//! number entries by their place, so after a change the same number can
//! name another entry. [`Handle::activate_entry`] sends `Event` `clicked`
//! only while the menu still has that entry as it was (the same kind,
//! label, toggle kind, and submenu or not), and refuses otherwise.
//! [`Handle::open_submenu`] and [`Handle::close_submenu`] tell the
//! application about submenus, many of which are filled only when opened.
//!
//! `ItemActivationRequested`, an application asking for its menu to be
//! shown (meant for a global menu bar answering a keyboard shortcut), is
//! not followed: a tray opens a menu when the user asks it to, and an
//! application opening one on its own would put up a popup the user did
//! not ask for.
//!
//! # Whom a call goes to
//!
//! Every call that expects a reply (reading the properties, and the menu's
//! `GetLayout`, its properties and `AboutToShow`) goes to the unique name
//! of the connection that owns the item, looked up when the call starts. The
//! bus never activates a unique name, so no read can start a service, even
//! for an item registered under a well-known name that has a service file
//! and has just left. An answer from a connection that no longer owns the
//! item, or from an object that is no longer its menu, says nothing about
//! it, whatever it says; the read the change asked for follows. Calls that
//! expect no reply (the activations, and the menu's `Event`s) go to the
//! name the item registered and carry `NO_AUTO_START`, which says the same
//! to the bus.
//!
//! # Untrusted input
//!
//! Every item is untrusted ([`Limits`]). Registrations are bounded, in all
//! and per connection. A read must arrive within its timeout and its reply
//! within its size, or it is refused as a whole: a pending item stays
//! pending, costing only its entry, and a reported one keeps what was last
//! read. Reads run concurrently, so a slow item delays only itself, and
//! each item is read no more often than its read interval allows.
//! Properties are decoded straight into their types; one of the wrong type
//! reads as absent. Strings are cleaned of control characters and bounded,
//! a tooltip's markup is parsed into plain text with no tags or entities
//! left in it, icon names checked to be names and not paths, and pixmaps
//! validated (their data must match their size, within bounds) and decoded
//! to RGBA.
//!
//! A menu is held to the same rules. Its layout is read straight into
//! entries, stopping at the first entry deeper or one more than its bounds
//! allow, and such a layout is refused whole, as is a reply over its size:
//! a menu is never shown with entries silently missing. Each field is
//! bounded on its own: a label is cleaned and truncated, an icon's PNG
//! (`icon-data`) is decoded within its bounds or dropped, shortcuts beyond
//! their count are dropped. Labels are text, not markup: the protocol
//! defines markup for none of an entry's properties, so `<b>` is shown as
//! written; underscores mark the mnemonic ([`Label`]). Hidden entries are
//! left out; nothing else is tidied, separators included.
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
//! Their menus differ too:
//!
//! | | libdbusmenu-glib (libayatana, GTK applications) | Qt | ksni |
//! |---|---|---|---|
//! | Entry ids | a counter per process, given once to each entry: the same for its life | a counter per process, given as each entry is made: the same for its life | the entry's place in the menu, all moved past the last when the layout changes: never reused |
//! | `children-display` | `submenu` while the entry has children | `submenu` for every submenu, empty or not | `submenu` while the entry has children |
//! | Toggles | | `checkmark`, or `radio` in an exclusive group; state 0 or 1 | `checkmark`, `radio`; state 0 or 1 |
//! | Icons | `icon-data`, a PNG of the GTK image | `icon-name` when the icon has a name, otherwise `icon-data`, a 16-pixel PNG | either or both, as the application gives |
//! | Shortcuts | one shortcut, modifiers then GDK's key name | one list per step of the key sequence: `Super`, `Control`, `Alt`, `Shift`, `Num`, then Qt's key name | as the application gives |
//! | Labels | | the first `&` made `_`; an underscore of the label's own is not doubled, and reads as a mnemonic | as the application gives |
//! | `AboutToShow` | answers false, then does its work | answers false; the application fills the menu then, and says it changed | answers false; says when the menu changed |
//! | Properties at their default | left out | `label`, `enabled` and `visible` always sent | left out |
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
mod menu;
mod model;
mod parse;
mod wire;

pub use deskrinch_bus::Bus;
pub use deskrinch_untrusted::{IconName, Image};
pub use host::{Activation, Config, Event, Handle, Host, OperationError, Orientation, StartError};
pub use item::{Category, Icon, Item, ItemId, Status, ToolTip};
pub use limits::Limits;
pub use menu::{
    Disposition, EntryId, EntryKind, Label, Menu, MenuEntry, MenuFailure, Shortcut, TextDirection,
    Toggle, ToggleState,
};
