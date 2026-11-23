//! What KWin says, read-only, for deskrinch's tests.
//!
//! Our suites read the Wayland protocols and the screen; what KWin itself
//! thinks of a window (its window type, above all, which KWin gives a layer
//! surface by its namespace alone, and which decides the effects it gets)
//! is in neither. This crate asks KWin.
//!
//! [`windows`] loads a KWin script through KWin's scripting interface
//! (`org.kde.kwin.Scripting` on `/Scripting`), runs it, and reads back, for
//! every window KWin has, the facts as KWin holds them ([`Window`]). The
//! script reports through `callDBus`, one call a window and a final call,
//! to an object this process serves under a name of its own on the same
//! bus, and is unloaded afterwards ([`script`]).
//!
//! # Only inside an isolated session
//!
//! KWin lets anything on its bus load and run a script: no grant is asked
//! for, unlike its screenshot interface (checked on KWin 6.6.7: an
//! ungranted caller loaded, ran and unloaded one). On the owner's own bus
//! that would run our scripts in the owner's KWin. So every function here
//! first checks that the process runs inside an isolated session of ours,
//! with KWin as its compositor ([`deskrinch_test_session::check`]), and
//! refuses, with [`Error::NotIsolated`] or [`Error::NotKwin`], before it
//! makes any connection: `cargo xtask session kwin` and
//! `cargo xtask compositor-tests kwin` run it.
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

mod error;
mod guard;
mod read;
pub mod script;
mod support;
mod window;
#[cfg(test)]
mod wiring_tests;

pub use error::Error;
pub use read::windows;
pub use support::version;
pub use window::{Skip, Window, WindowType};
