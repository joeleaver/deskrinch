//! The workspace switcher's model, without a UI: which workspaces have a
//! button, in what order, labelled and marked how, and which buttons fit in
//! the room the panel gives the switcher.
//!
//! It follows the workspaces `deskrinch-workspaces` reports. The component
//! turns each of that crate's events into a [`Change`] (its [`Entry`]
//! carries what the switcher uses of a workspace) and draws the [`Run`]s the
//! model gives. The model is generic over the workspace's and the group's
//! ids, so it is tested without a compositor.
//!
//! # Which workspaces have a button
//!
//! Every workspace but those the compositor asks a switcher not to show
//! (hidden). The buttons come in runs, one per group of workspaces:
//!
//! - Where the compositor gives the workspaces coordinates in its layout
//!   (KWin's rows and columns), a run is in their order, row by row; where
//!   it gives none, in the order the workspaces were reported.
//! - A compositor whose workspaces are shared by every output (KWin, and
//!   labwc) has one group, and the switcher one run. One with workspaces per
//!   output has a group per output; the runs are then ordered by their
//!   outputs' names, so that no output's workspaces are hidden while one
//!   panel serves every output. No compositor the shell is tested against
//!   has several groups, so that case is covered by this crate's tests
//!   alone. Once each output has a panel of its own, each panel shows its
//!   own output's group.
//! - Workspaces that belong to no group the compositor has reported form a
//!   run of their own, last.
//!
//! # What a button shows
//!
//! Its position: the workspace's place in its run, from 1 ([`Button::label`]),
//! not the compositor's name for it. Every compositor the shell is tested
//! against names workspaces "a word and a number" ("Desktop 2",
//! "Workspace 2"), so the number is what tells them apart, and a position
//! takes a fraction of a name's width. Where a compositor numbers its
//! workspaces otherwise than their order (a workspace named "5" third in
//! the order), the button shows 3: the place a user sees, not the
//! compositor's number. The name is not shown at all yet: a tooltip would
//! be drawn inside the bar, over the other buttons, until rinch can show
//! one outside the panel's surface.
//!
//! The workspace shown on the run's outputs is marked current
//! ([`Button::current`]). A click asks to activate a workspace the compositor
//! allows that on ([`Button::activatable`]); the compositor decides, and its
//! answer arrives as a change that moves the mark.
//!
//! # When the buttons do not fit
//!
//! [`fit`]: each run's buttons, all of one width, fit side by side in the
//! run's share of the room; when they do not, the workspaces furthest from
//! the current one are left out and counted at either end, never the
//! current one.

mod fit;
mod switcher;

pub use fit::{Fitted, Room, fit};
pub use switcher::{Button, Change, Entry, Run, Switcher};
