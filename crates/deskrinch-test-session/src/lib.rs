//! Where a test of deskrinch's may act: inside an isolated session, on a CI
//! runner, or neither. What an isolated test session is, and whether this
//! is a CI runner, each in one place.
//!
//! `cargo xtask session` and `cargo xtask compositor-tests` run programs in
//! sessions of their own: a nested compositor, a private D-Bus daemon, and
//! a root directory under `/tmp` holding the session's home and runtime
//! directories. A test that talks to a session service (a compositor's bus
//! interfaces, the notification daemon) must run only inside one, never
//! against the owner's real session. This crate is the one home of what
//! tells the two apart:
//!
//! - the variables a session gives its clients, by name
//!   ([`COMPOSITOR_VARIABLE`], [`SESSION_DIR_VARIABLE`],
//!   [`COMPOSITOR_PID_VARIABLE`], [`SESSION_GROUP_VARIABLE`]): the session
//!   sets them by these names and every reader reads them by these names;
//! - a session root's layout and the check that a directory is one
//!   ([`root`]);
//! - the private bus's address and the check that an address is one
//!   ([`bus`]);
//! - the check that a process runs inside a session, from those three
//!   ([`check`]), with values passed in, so that its refusals are tested
//!   without touching the environment;
//! - whether this is a CI runner, for what may run only on one, and for
//!   what must not run on one ([`ci`]).
//!
//! It has no dependencies, so that any test crate can take it: a Wayland
//! test client as much as a bus client.
//!
//! # Testing a refusal
//!
//! A test that a program refuses outside a session is also run with the
//! refusal broken (its mutant), to show that the test catches that; then
//! the program does what the refusal kept it from. So the test runs the
//! program where doing it reaches nothing: a child with no display, an
//! empty runtime directory and a bus address that leads nowhere
//! (`deskrinch_test_dbus::session::outside` makes one, for any crate's
//! tests). Mutants of a guard are run only where the guard's failure is
//! harmless. A guard whose test cannot be made harmless has its mutant run
//! only inside an isolated session (`cargo xtask session`) or in CI.

pub mod bus;
mod check;
pub mod ci;
pub mod root;

pub use check::{Environment, Isolated, NotIsolated, check};

/// The variable naming the session's compositor (`kwin`, `sway` or
/// `labwc`).
pub const COMPOSITOR_VARIABLE: &str = "DESKRINCH_TEST_COMPOSITOR";

/// The variable naming the session's root directory.
pub const SESSION_DIR_VARIABLE: &str = "DESKRINCH_SESSION_DIR";

/// The variable naming the session's compositor's process id, for a client
/// that ends the compositor under its other clients. The compositor leads
/// the session's process group, so this is that group's id too.
pub const COMPOSITOR_PID_VARIABLE: &str = "DESKRINCH_SESSION_COMPOSITOR_PID";

/// The variable naming the session's process group, given by
/// `cargo xtask compositor-tests` to a test runner that ends the session
/// by signalling that group (and so runs outside it).
pub const SESSION_GROUP_VARIABLE: &str = "DESKRINCH_TEST_SESSION_GROUP";

/// The variable naming the session's bus: D-Bus's own.
pub const BUS_VARIABLE: &str = "DBUS_SESSION_BUS_ADDRESS";
