//! What the shell's protocol crates (`deskrinch-window-management`,
//! `deskrinch-workspaces`) share: a Wayland connection of their own,
//! separate from any UI toolkit's; a thread per crate that dispatches its
//! queue; and the compositor's outputs, named and laid out.
//!
//! - [`Client`] is one connection to the compositor. It is cheap to clone,
//!   and several crates may use one: each makes its own event queue and
//!   dispatch thread on it (`connect_with` in each crate). A component
//!   using two of them, as the panel does, then holds one connection beside
//!   its toolkit's instead of two. The connection is shared in full: a
//!   protocol error caused by one crate, or the compositor going away, ends
//!   every crate's stream on it.
//! - [`DispatchThread`] runs one event queue on a thread it owns until it is
//!   dropped or the connection fails, which it reports through [`Ended`]
//!   once.
//! - [`Outputs`] binds `wl_output` and `zxdg_output_v1` into a crate's own
//!   dispatch state ([`OutputsHandler`]) and tracks each output's name and
//!   logical rectangle.
//!
//! No async runtime is needed, and nothing here depends on rinch.

mod client;
mod dispatch;
mod outputs;

pub use client::Client;
pub use dispatch::{DispatchThread, Ended, Wake};
pub use outputs::{OutputGlobal, Outputs, OutputsHandler, Rect, layout_available, names_available};
