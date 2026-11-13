//! The compositor's workspaces: listed, followed, and switched between,
//! behind one trait.
//!
//! A client sees the compositor's workspaces only through a protocol the
//! compositor offers for it. This crate speaks two:
//!
//! - `ext_workspace_v1`, the standard protocol, offered by labwc (0.9) and
//!   to be offered by deskrinch's own compositor;
//! - `org_kde_plasma_virtual_desktop_management`, KWin's, offered to every
//!   client (KWin grants it without a desktop file). It is bound at
//!   version 2, the newest `wayland-protocols-plasma` carries; KWin 6.6
//!   offers version 3, which adds `request_move_virtual_desktop`
//!   (reordering desktops).
//!
//! [`connect`] picks whichever the compositor offers (the standard one when
//! both are), and the caller sees only the [`Workspaces`] trait. sway (1.11)
//! offers neither: its workspaces are reachable only through its IPC, and
//! [`connect`] fails with [`ConnectError::Unsupported`]; this crate will
//! use the standard protocol when sway offers it, and has no IPC backend.
//!
//! The overview (a view of every workspace at once) is the compositor's
//! own; this crate neither draws nor triggers one.
//!
//! # The model
//!
//! - A [`Workspace`] has an id, a name, a [`WorkspaceState`] (active,
//!   urgent, hidden), the [`Group`] it belongs to, its coordinates in the
//!   compositor's layout of workspaces, and the actions the compositor
//!   allows on it now.
//! - A [`Group`] is a set of workspaces sharing outputs, and names the
//!   outputs. KWin's desktops are shared by every output: one group on all
//!   of them. `ext_workspace_v1` has a group per output where the
//!   compositor keeps workspaces per output; labwc has one group.
//! - KWin's desktop order and rows become `[row, column]` coordinates, so
//!   both protocols report a layout in one field; labwc sends none.
//! - The subscriber passed to [`connect`] hears every change as an
//!   [`Event`], one batch per protocol `done`: a group is reported by
//!   `GroupChanged` (its whole new state) until `GroupRemoved`; a workspace
//!   is `Added` once, then `Changed` (its whole new state), then `Removed`.
//!   `Ended` closes the stream.
//! - [`Workspaces::perform`] requests an [`Action`]: activate. The
//!   compositor decides; its decision arrives as an event. Creating and
//!   removing workspaces are not offered: no compositor this crate is
//!   tested against grants them on the standard protocol.
//!
//! Protocols differ in what they report and allow, and
//! [`Workspaces::capabilities`] says what this connection has: the state
//! flags it reports (KWin's reports only active), whether groups name
//! outputs, whether workspaces may carry coordinates, and the actions it can
//! carry. A flag that is not reported is never set. Each workspace's own
//! [`actions`](Workspace::actions) say what the compositor allows on it;
//! an action outside them fails with [`ActionError::NotAllowed`].
//!
//! Everything the compositor sends is bounded: names are cleaned of control
//! characters and cut to 256 bytes (`deskrinch-untrusted`'s rule), at most
//! 256 workspaces and 64 groups are listed, coordinates have at most eight
//! dimensions, and KWin's rows at most 64.
//!
//! # Threads and connections
//!
//! No async runtime is needed. [`connect`] opens a Wayland connection of
//! its own, separate from any UI toolkit's; [`connect_with`] uses a
//! [`Client`]'s connection instead, which `deskrinch-window-management` may
//! share, so that a component using both holds one connection beside its
//! toolkit's. Either way the crate dispatches an event queue of its own on a
//! thread it owns. A shared connection is shared in full: a protocol error
//! raised by any of its users, or the compositor going away, ends every
//! stream on it.
//!
//! The subscriber runs on the dispatch thread (and, for what is already
//! there, on the thread calling [`connect`] before it returns), so a UI
//! forwards events into its own loop. The [`WorkspaceManager`] handle may
//! be used from any thread.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_workspaces::{Action, Event, Workspaces, connect};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let (sender, events) = std::sync::mpsc::channel();
//! let manager = connect(move |event| {
//!     // Hand the event to the UI's own loop.
//!     let _ = sender.send(event);
//! })?;
//!
//! for workspace in manager.workspaces() {
//!     println!("{}: {:?}", workspace.name, workspace.state);
//! }
//! if let Some(second) = manager.workspaces().get(1) {
//!     manager.perform(second.id, Action::Activate)?;
//! }
//! for event in events {
//!     if let Event::Changed(workspace) = event {
//!         println!("{} is now {:?}", workspace.name, workspace.state);
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
//! runs them inside nested, headless KWin, sway and labwc sessions.

mod backend;
mod capabilities;
mod error;
mod manager;
mod model;
mod workspace;

pub use capabilities::{Action, Actions, Capabilities, Protocol};
pub use deskrinch_wayland_client::Client;
pub use error::{ActionError, ConnectError, EndReason};
pub use manager::{WorkspaceManager, Workspaces, connect, connect_with};
pub use workspace::{Event, Group, GroupId, Workspace, WorkspaceId, WorkspaceState};
