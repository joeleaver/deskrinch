//! `cargo xtask systemd-scope-tests`: runs the desktop-entries test that
//! launches into a real transient scope of the user's systemd instance.
//!
//! The test is ignored by `cargo test` because it uses the real session:
//! the systemd user instance on the session bus. It starts only the test's
//! own recorder script, in a transient scope that it stops again.

use std::path::Path;
use std::process::Command;

use crate::Outcome;
use crate::error::Error;

/// The ignored test, by its full path in the integration-test binary.
const TEST: &str = "systemd::a_launch_and_its_children_land_in_a_real_transient_scope";

/// Runs the test with `cargo test` in `root`, inheriting this process's
/// session.
pub(crate) fn run(root: &Path) -> Result<Outcome, Error> {
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "test",
            "--locked",
            "-p",
            "deskrinch-desktop-entries",
            "--test",
            "integration",
            "--",
            "--ignored",
            "--exact",
            TEST,
        ])
        .status()
        .map_err(|source| Error::Spawn {
            program: "cargo".into(),
            source,
        })?;
    if status.success() {
        println!("systemd-scope-tests: passed");
        Ok(Outcome::Passed)
    } else {
        eprintln!("systemd-scope-tests: failed ({status})");
        Ok(Outcome::Failed)
    }
}
