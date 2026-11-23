//! The shared connection's rows: on the Rust backend and on libwayland.

use super::{ALL, Output, PLAIN, Suite};
use crate::compositor_tests::build::{Backend, Target};

const PACKAGE: &str = "deskrinch-wayland-client";

/// The shared connection's one thread: states attached and detached from
/// several threads at once, and how the thread and its states end.
pub(super) const WAYLAND_CLIENT: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::Rust,
    },
    grant: None,
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[],
    output: Output::Captured,
};

/// The two suites that exercise the shared connection, again on
/// libwayland: what every binary that links rinch gets (winit selects
/// `wayland-client`'s `system` backend), while these crates alone get the
/// Rust one. Window management's session-ending test runs once, last, on
/// the Rust backend: a session ends only once.
pub(super) const WAYLAND_CLIENT_ON_LIBWAYLAND: Suite = Suite {
    target: Target {
        package: PACKAGE,
        test: "compositor",
        backend: Backend::System,
    },
    grant: None,
    compositors: ALL,
    passes: PLAIN,
    renders: false,
    programs: &[],
    filter: &[],
    output: Output::Captured,
};
