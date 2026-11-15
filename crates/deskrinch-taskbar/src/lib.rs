//! The taskbar's model, without a UI: which windows have a button, in what
//! order and how marked, what a click on one asks of the compositor, and
//! which buttons fit in the room the panel gives it.
//!
//! It follows the window list `deskrinch-window-management` reports. The
//! component turns each of that crate's events into a [`Change`] (its
//! [`TaskWindow`] carries what the taskbar uses of a window), gives the
//! model the pointer's presses and releases on the buttons, and does what
//! the model answers. The model is generic over the window's id, so it is
//! tested without a compositor.
//!
//! # Which windows have a button
//!
//! - Every window that does not ask to be left off a taskbar
//!   ([`WindowState::SKIP_TASKBAR`], where the compositor reports it) and
//!   has no parent, on every output. A window with a parent (a dialog)
//!   belongs to its parent's button.
//! - A button stands for its window and that window's dialogs, and theirs:
//!   it is active when one of them is, and marked for attention when one of
//!   them asks for it ([`WindowState::DEMANDS_ATTENTION`]). It is dimmed
//!   while its own window is minimized.
//! - A window whose parent is not a window the model knows has a button of
//!   its own, so a dialog left behind stays reachable.
//! - The buttons are in the order the windows were added: the order they
//!   opened, and for the windows already open when the taskbar connected,
//!   the order the compositor reported them in.
//!
//! # What a click does
//!
//! A click is a press and a release of the same button on the same window's
//! button ([`deskrinch_gestures::Click`]); anything else does nothing.
//!
//! - The primary button on a minimized window: [`Action::Restore`] and then
//!   [`Action::Activate`], where the connection can restore; `Activate` alone
//!   where it cannot.
//! - The primary button on the active button: [`Action::Minimize`] of the
//!   button's own window, where the connection can minimize; nothing where
//!   it cannot, since the window is already active.
//! - The primary button on any other: `Activate`.
//! - The middle button: [`Action::Close`].
//!
//! An action the connection cannot request is never asked for
//! ([`Taskbar::connected`] says which it can).
//!
//! # When the compositor goes
//!
//! [`Change::Ended`] (the compositor stopped reporting windows, or the
//! connection was lost) empties the taskbar, and is answered with
//! [`Answer::Ended`] once, for the component to log why; after it, nothing
//! more is heard or done. A taskbar that could not connect at all is
//! [`Taskbar::unavailable`], and shows nothing.
//!
//! # When the buttons do not fit
//!
//! [`fit`]: the buttons share the room up to a widest size, with their
//! titles cut to fit. Below a narrowest titled size they shrink to the icon
//! alone. If those still do not fit, the oldest windows that are not active
//! are left out, and the room for a count of them is kept at the end.
//!
//! # Threads
//!
//! The model is plain data. The component keeps it where the
//! window-management crate's events arrive, and applies each event there,
//! in the order they come, so that none is lost or reordered on the way to
//! the UI; the UI is handed the [`Button`]s that result.

mod fit;
mod taskbar;

pub use deskrinch_window_management::{Action, Actions, EndReason, WindowState};
pub use fit::{Fit, Form, Room, fit};
pub use taskbar::{Answer, Button, Change, Input, PointerButton, TaskWindow, Taskbar};
