//! A shell component's binary under test in the nested session: started
//! with the renderer the run asks for, its output watched, and ended with
//! the test; and one home for reading what rinch says in that output.
//!
//! - [`Component`] starts a binary (the panel's, the launcher's) with
//!   `RINCH_PERF` frame lines, rinch's renderer line and wgpu's adapter line
//!   switched on, watches both of its streams ([`Watch`]), waits for its
//!   first frame and the renderer that presented it, and kills it when
//!   dropped.
//! - [`Renderer`] is the run's renderer, which `cargo xtask
//!   compositor-tests` names in [`RENDERER_VARIABLE`]; a test asserts that
//!   it is what presented, so a silent fallback fails.
//! - [`log`] reads rinch's and wgpu's lines, and [`perf`] every kind of
//!   rinch's `RINCH_PERF` line (each window's presented and prepared
//!   frames, windows opening and closing); `cargo xtask measure` reads them
//!   through it too, so a change of wording in rinch is caught in one
//!   place. [`Watch`] keeps the main window's frames as it always did, and
//!   every window's beside them.
//!
//! Like the other test-support crates, it is for nested sessions only: the
//! component it starts connects to whatever `WAYLAND_DISPLAY` and the
//! session bus name. Its entry point, [`Component::start`], refuses (it
//! panics, before it spawns anything) unless it runs in a nested session
//! that `cargo xtask compositor-tests` started, as
//! `deskrinch_test_client::Compositor::current` tells; so a test that
//! forgets its own guard still puts nothing on the real desktop.

mod component;
pub mod log;
pub mod perf;
mod watch;
mod windows;

pub use component::{Component, HUNG_AFTER, RENDERER_VARIABLE, RINCH_RENDERER_VARIABLE, Renderer};
pub use perf::{Opened, PerfLine, PerfWindow};
pub use watch::{LAST_WORDS, Presented, Watch};
pub use windows::{Mark, SinceMark, WindowState};
