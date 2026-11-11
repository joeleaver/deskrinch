//! The application launcher, deskrinch's second component.
//!
//! The launcher runs for the whole session as one process with one layer
//! surface: in the overlay layer, centred, taking all keyboard input while
//! it is shown, and unmapped while hidden. It is summoned through its own
//! session-bus interface, finds installed applications as the user types,
//! and starts the chosen one.
//!
//! This crate is built up in parts; this is the first:
//!
//! - [`Model`]: what the launcher shows and what a key does: the query, the
//!   ranked results ([`Row`]s) and the selection. Pure: it needs no
//!   display, bus or files, and is tested on its own.
//!
//! # How the launcher behaves
//!
//! These are the model's rules; the process follows the [`Action`] each
//! [`Input`] gives.
//!
//! - The results are the applications shown in menus that match the query,
//!   by `deskrinch-desktop-entries`' search ranking. An empty query matches
//!   nothing.
//! - The first result is selected whenever the query changes. Up and Down
//!   move the selection, stopping at the first and the last result.
//! - When the applications change while the launcher is shown (one is
//!   installed or removed), the results are searched again and the
//!   selection stays on the same application if it is still there.
//! - Enter starts the selected application; with none selected it does
//!   nothing. Escape hides the launcher.
//! - Losing the keyboard while shown hides the launcher, whatever the query
//!   says. A compositor moves the keyboard elsewhere when the user turns to
//!   another window, and KWin moves it to an application the launcher
//!   started as soon as that maps: either way the launcher has been left,
//!   and it never takes the keyboard back.
//! - The launcher starts empty each time it is shown: the process
//!   [`clear`](Model::clear)s the model when it hides.

mod model;

pub use model::{Action, Input, Model, Row};
