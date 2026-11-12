//! The crate against a real compositor: nested KWin, sway or labwc, started
//! by `cargo xtask compositor-tests`, which runs these one at a time (they
//! share the compositor's focus) and [`session_end`] last, on its own (it
//! ends the session). Each test opens its own windows, in clients of their
//! own, and finds them by an app id no other window has.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

#[cfg(test)]
mod actions;
#[cfg(test)]
mod listing;
#[cfg(test)]
mod placement;
#[cfg(test)]
mod session_end;
#[cfg(test)]
mod shared;
