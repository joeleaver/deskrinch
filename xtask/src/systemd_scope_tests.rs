//! `cargo xtask systemd-scope-tests`: runs the desktop-entries test that
//! launches into a real transient scope of the user's systemd instance.
//!
//! The test is ignored by `cargo test` because it uses the real session:
//! the systemd user instance on the session bus. It starts only the test's
//! own recorder script, in a transient scope that it stops again.
//!
//! It runs only on a CI runner, and refuses anywhere else before it starts
//! anything: on a developer's machine the unit would be started in their
//! own systemd. The test refuses too, for whoever runs it with `--ignored`.

use std::path::Path;

use deskrinch_test_session::ci::Ci;

use crate::Outcome;
use crate::error::{Error, only_on_a_runner};

/// The ignored test, by its full path in the integration-test binary.
const TEST: &str = "systemd::a_launch_and_its_children_land_in_a_real_transient_scope";

/// Runs the test with `cargo test` in `root`, inheriting this process's
/// session.
pub(crate) fn run(root: &Path) -> Result<Outcome, Error> {
    on_ci(&Ci::read())?;
    let status = crate::cargo::command()
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

/// Goes on only on a CI runner, as `ci` says.
fn on_ci(ci: &Ci) -> Result<(), Error> {
    only_on_a_runner(
        "cargo xtask systemd-scope-tests (which starts a unit in the user's systemd)",
        ci,
    )
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn refuses_anywhere_but_a_runner() {
        for value in [None, Some(""), Some("false"), Some("TRUE")] {
            let ci = Ci::from_value(value.map(OsStr::new));
            let refused = on_ci(&ci).unwrap_err().to_string();
            assert!(
                refused.starts_with(
                    "cargo xtask systemd-scope-tests (which starts a unit in the user's systemd) \
                     runs only on a CI runner"
                ),
                "{value:?}: {refused}"
            );
        }
        assert!(on_ci(&Ci::from_value(Some(OsStr::new("true")))).is_ok());
    }
}
