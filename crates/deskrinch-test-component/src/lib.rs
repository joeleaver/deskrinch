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
//! - [`log`] reads rinch's and wgpu's lines; `cargo xtask measure` reads
//!   them through it too, so a change of wording in rinch is caught in one
//!   place.
//!
//! Like the other test-support crates, it is for nested sessions only
//! (`cargo xtask compositor-tests`, `cargo xtask session`): the component
//! it starts connects to whatever `WAYLAND_DISPLAY` and the session bus
//! name.

mod component;
pub mod log;
mod watch;

pub use component::{Component, RENDERER_VARIABLE, Renderer};
pub use watch::{Presented, Watch};
