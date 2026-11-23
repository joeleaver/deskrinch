//! What KWin says, and what a test may ask of it, inside an isolated
//! session, for deskrinch's tests: the one home of what we know of KWin's
//! bus (its names, paths, its spelling of a critical notification, who owns
//! its shortcut service).
//!
//! Our suites read the Wayland protocols and the screen; what KWin itself
//! thinks of a window (its window type, above all, which KWin gives a layer
//! surface by its namespace alone, and which decides the effects it gets)
//! is in neither. This crate asks KWin. What a test changes in KWin (its
//! virtual desktops, a shortcut invoked) goes through a handle of its own,
//! [`KwinActions`], so that a function given a [`Kwin`] only reads.
//!
//! Every question is asked through a [`Kwin`] handle, made only by
//! [`Kwin::here`] (this process runs inside an isolated KWin session of
//! ours) or [`Kwin::on`] (the address is a private bus of ours):
//!
//! - [`Kwin::windows`] loads a KWin script through KWin's scripting
//!   interface (`org.kde.kwin.Scripting` on `/Scripting`), runs it, and
//!   reads back, for every window KWin has, the facts as KWin holds them
//!   ([`Window`]). The script reports through `callDBus`, one call a window
//!   and a final call, to an object this process serves under a name of its
//!   own on the same bus, and is unloaded afterwards.
//! - [`Kwin::version`] and [`Kwin::compositing_type`] read KWin's support
//!   information.
//! - [`Kwin::active_effects`] and [`Kwin::effect_loaded`] ask its effects.
//! - [`Kwin::kwin_shortcut_names`] asks its shortcut service.
//! - [`Kwin::virtual_desktops`], [`Kwin::current_virtual_desktop`] and
//!   [`Kwin::current_desktop`] read its desktops.
//!
//! [`KwinActions`], made the same two ways, lays out, switches, adds and
//! removes virtual desktops and invokes KWin's shortcuts.
//!
//! # Only on a bus of ours
//!
//! KWin lets anything on its bus load and run a script: no grant is asked
//! for, unlike its screenshot interface (checked on KWin 6.6.7: an
//! ungranted caller loaded, ran and unloaded one). On the owner's own bus
//! that would run our scripts in the owner's KWin. So the handle's two
//! makers refuse, with [`Error::NotIsolated`], [`Error::NotKwin`] or
//! [`Error::NotOurBus`], before any connection, and every connection is to
//! the address the handle was proved for: `cargo xtask session kwin` and
//! `cargo xtask compositor-tests kwin` give a process such a session, and
//! xtask and the desktop's tests hold the address of the session they
//! started.
//!
//! # Telling windows apart
//!
//! KWin's script API gives a toplevel's title as its caption and its app
//! id as its resource class. A layer surface has neither: its caption,
//! `desktopFileName` and `windowRole` are empty, and its resource class and
//! name are its process's name; no property carries its namespace (KWin
//! 6.6.7, probed with the test client's overlay). So a test tells its
//! layer surfaces apart by what it controls: a size unique to each, with
//! its own process id ([`Window::pid`], [`Window::size`]).

mod actions;
mod desktops;
mod effects;
mod error;
mod kwin;
mod read;
mod script;
mod shortcuts;
mod support;
mod window;
#[cfg(test)]
mod wiring_tests;

pub use actions::KwinActions;
pub use desktops::VirtualDesktop;
pub use error::Error;
pub use kwin::Kwin;
pub use window::{Skip, Window, WindowType};
