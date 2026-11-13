//! The compositor's list of windows, and actions on them, behind one trait.
//!
//! On Wayland a client cannot see other clients' windows except through a
//! protocol the compositor offers for it. This crate speaks two:
//!
//! - `zwlr_foreign_toplevel_management_v1`, offered by wlroots compositors
//!   (sway and others) and by deskrinch's own compositor;
//! - `org_kde_plasma_window_management`, offered by KWin, and only to
//!   clients KWin has granted it to, by an installed desktop file that lists
//!   [`KWIN_INTERFACE`] (the `deskrinch-kwin-access` crate says how KWin
//!   decides, and writes the file).
//!
//! [`connect`] picks whichever the compositor offers (the wlroots protocol
//! when both are), and the caller sees only the [`WindowManagement`] trait.
//!
//! # The model
//!
//! - A [`Window`] has an id, a title, an application id, a [`WindowState`]
//!   (active, minimized, maximized, fullscreen, demanding attention,
//!   skipping the taskbar), the outputs it is on, and its parent (the
//!   window a dialog belongs to).
//! - The subscriber passed to [`connect`] hears every change as an
//!   [`Event`]: a window is `Added` once, then `Changed` (carrying its whole
//!   new state), then `Removed`. `Ended` closes the connection's stream.
//! - [`WindowManagement::windows`] is the list as last reported.
//! - [`WindowManagement::perform`] requests an [`Action`]: activate,
//!   minimize, restore or close. The compositor decides; its decision
//!   arrives as an event.
//!
//! Protocols differ in what they report and allow, and
//! [`WindowManagement::capabilities`] says what this connection has: the
//! state flags it reports, whether it reports outputs and parents, and its
//! actions. A flag that is not reported is never set; outputs and parents
//! that are not reported stay empty; an action that is missing fails with
//! [`ActionError::Unsupported`]. The wlroots protocol, for instance, does not
//! report attention or taskbar hints. KWin's reports a window's geometry
//! rather than its outputs, and this crate works the outputs out from the
//! output layout, so the caller sees outputs either way.
//!
//! # Threads
//!
//! No async runtime is needed. [`connect`] opens a Wayland connection of
//! its own, separate from any UI toolkit's; [`connect_with`] uses a
//! [`Client`]'s connection instead, which other crates
//! (`deskrinch-workspaces`) may share, so that a component using both holds
//! one connection beside its toolkit's. Either way the crate's state is
//! attached to a [`Client`], whose one thread reads the connection and
//! dispatches every state on it (`deskrinch-wayland-client` says why one
//! thread). A shared connection is shared in full: a protocol error raised
//! by any of its users, the compositor going away, or a panic in any
//! subscriber on it ends every stream on it ([`Event::Ended`]).
//!
//! The subscriber runs on the client's thread, the windows already open
//! included (before [`connect`] returns), so a UI forwards events into its
//! own loop. The [`WindowManager`] handle may be used from any thread.
//!
//! # Dropping the handle
//!
//! Dropping the [`WindowManager`] tells the compositor to stop: every
//! window's handle is destroyed, the outputs and the seat released, and
//! under wlroots' protocol the manager stopped; the state is dropped once
//! the compositor has `finished` with it. KWin's window-management manager
//! has no destructor and no `stop`, so under KWin the state, on a
//! connection shared through [`connect_with`], is kept and its events
//! thrown away for as long as the connection lasts: every detach leaves
//! that residue, on both sides, until the connection closes. A component
//! that would connect and drop repeatedly keeps one manager, or uses
//! [`connect`], whose connection closes with the handle.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_window_management::{Action, Event, WindowManagement, connect};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (sender, events) = std::sync::mpsc::channel();
//! let manager = connect(move |event| {
//!     // Hand the event to the UI's own loop.
//!     let _ = sender.send(event);
//! })?;
//!
//! for window in manager.windows() {
//!     println!("{}: {} ({})", window.id, window.title, window.app_id);
//! }
//! for event in events {
//!     if let Event::Added(window) = event {
//!         if manager.capabilities().supports(Action::Activate) {
//!             manager.perform(window.id, Action::Activate)?;
//!         }
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Testing against compositors
//!
//! The unit tests need no compositor. The tests in `tests/` drive a real one
//! and are ignored by a plain `cargo test`; `cargo xtask compositor-tests`
//! runs them inside nested, headless KWin, sway and labwc sessions with two
//! outputs each and their own runtime, data and configuration directories,
//! touching nothing in the real session.

mod backend;
mod capabilities;
mod error;
mod layout;
mod manager;
mod model;
mod window;

pub use capabilities::{Action, Actions, Capabilities, Protocol};
pub use deskrinch_wayland_client::{Client, StateCounts};
pub use error::{ActionError, ConnectError, EndReason};
pub use manager::{WindowManagement, WindowManager, connect, connect_with};
pub use window::{Event, Window, WindowId, WindowState};

/// The restricted global this crate needs from KWin: the name a KWin grant
/// for an executable that uses this crate lists among its Wayland
/// interfaces (`deskrinch_kwin_access::DesktopEntry`).
pub const KWIN_INTERFACE: &str = "org_kde_plasma_window_management";
