//! The compositor's list of windows, and actions on them, behind one trait.
//!
//! On Wayland a client cannot see other clients' windows except through a
//! protocol the compositor offers for it. This crate speaks two:
//!
//! - `zwlr_foreign_toplevel_management_v1`, offered by wlroots compositors
//!   (sway and others) and by deskrinch's own compositor;
//! - `org_kde_plasma_window_management`, offered by KWin, and only to
//!   clients KWin has granted it to (see [`kwin_access`]).
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
//! No async runtime is needed. The crate opens a Wayland connection of its
//! own, separate from any UI toolkit's, and dispatches it on a thread it
//! owns. The subscriber runs on that thread (and, for the windows already
//! open, on the thread calling [`connect`] before it returns), so a UI
//! forwards events into its own loop. The [`WindowManager`] handle may be
//! used from any thread.
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
pub mod kwin_access;
mod layout;
mod manager;
mod model;
mod window;

pub use capabilities::{Action, Actions, Capabilities, Protocol};
pub use error::{ActionError, ConnectError, EndReason};
pub use manager::{WindowManagement, WindowManager, connect};
pub use window::{Event, Window, WindowId, WindowState};
