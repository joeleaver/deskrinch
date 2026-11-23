//! A child of a refusal test, run outside any session where nothing could
//! be reached if the refusal failed: what the tests of xtask's guards share.
//!
//! A test that a guard refuses must be harmless when the guard does not
//! refuse, as a test is run with its guard broken to show that it catches
//! that. The child is isolated as a session would isolate it
//! ([`deskrinch_test_dbus::session::isolate`]) on a temporary root of its
//! own, with no session there: no `WAYLAND_DISPLAY`, `WAYLAND_SOCKET` or
//! `DISPLAY`, an `XDG_RUNTIME_DIR` that is empty, a session bus address
//! under the root that leads nowhere, and none of the variables `cargo
//! xtask session` names its session with. With the guard gone, a child
//! finds no compositor and no bus to connect to, and fails there.

use std::path::Path;
use std::process::Command;

use deskrinch_test_dbus::session::{create_dirs, isolate};
use deskrinch_test_session::{
    BUS_VARIABLE, COMPOSITOR_PID_VARIABLE, COMPOSITOR_VARIABLE, SESSION_DIR_VARIABLE,
    SESSION_GROUP_VARIABLE,
};
use tempfile::TempDir;

/// Points `command` at nothing, as the module's doc says; the root it is
/// pointed under, which must outlive the child.
pub(crate) fn outside(command: &mut Command) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    create_dirs(root.path()).unwrap();
    isolate(command, root.path());
    for variable in [
        COMPOSITOR_VARIABLE,
        SESSION_DIR_VARIABLE,
        COMPOSITOR_PID_VARIABLE,
        SESSION_GROUP_VARIABLE,
    ] {
        command.env_remove(variable);
    }
    command.env(BUS_VARIABLE, nowhere(root.path()));
    root
}

/// A session bus address under `root` with no socket at it.
fn nowhere(root: &Path) -> String {
    format!("unix:path={}/no-bus", root.display())
}
