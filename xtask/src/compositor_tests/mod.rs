//! `cargo xtask compositor-tests`: runs the window-management crate's
//! compositor tests inside nested, headless compositors.
//!
//! For each compositor it builds the crate's test binaries, starts an
//! isolated [`Session`], and runs the ignored tests there. Under KWin the
//! `compositor` binary is granted the window-management protocol the way an
//! installed deskrinch is, by a desktop file in the session's data directory
//! (KWin's check stays on), and `kwin_not_granted` checks that a binary
//! without one is refused. The `session_end` tests end the session under a
//! connected manager, so they run last.

mod build;

use std::path::Path;

use deskrinch_window_management::kwin_access::DesktopEntry;

use self::build::TestBinaries;
use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, Session};

/// How many outputs the sessions have, side by side: the tests move windows
/// between two.
const OUTPUTS: u32 = 2;

/// The module of the `compositor` tests that ends the session, so runs last.
const SESSION_END: &str = "session_end::";

/// The desktop file id the `compositor` test binary is granted under.
const DESKTOP_ID: &str = "org.deskrinch.WindowManagementTests";

/// Runs the tests in each of `compositors` (all of them when empty).
pub(crate) fn run(root: &Path, compositors: &[Compositor]) -> Result<Outcome, Error> {
    let compositors = if compositors.is_empty() {
        &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc][..]
    } else {
        compositors
    };
    let binaries = build::test_binaries(root)?;
    let mut failed = Vec::new();
    for &compositor in compositors {
        println!("compositor-tests: {}", compositor.name());
        if !run_in(compositor, &binaries)? {
            failed.push(compositor.name());
        }
    }
    if failed.is_empty() {
        println!("compositor-tests: passed");
        Ok(Outcome::Passed)
    } else {
        eprintln!("compositor-tests: failed in {}", failed.join(", "));
        Ok(Outcome::Failed)
    }
}

/// Runs the tests in one fresh session of `compositor`. Returns whether they
/// passed; on failure, prints the compositor's log.
fn run_in(compositor: Compositor, binaries: &TestBinaries) -> Result<bool, Error> {
    let session = Session::start(compositor, OUTPUTS, None, |data_home| match compositor {
        Compositor::Kwin => grant(&binaries.compositor, data_home),
        Compositor::Sway | Compositor::Labwc => Ok(()),
    })?;
    let mut passed = session.run_tests(&binaries.compositor, &["--skip", SESSION_END])?;
    if compositor == Compositor::Kwin {
        passed &= session.run_tests(&binaries.kwin_not_granted, &[])?;
    }
    // Last: it ends the session.
    passed &= session.run_tests(&binaries.compositor, &[SESSION_END])?;
    if !passed {
        eprintln!(
            "compositor-tests: {} log:\n{}",
            compositor.name(),
            session.log()
        );
    }
    session.end()?;
    Ok(passed)
}

/// Installs the desktop file that grants `binary` KWin's window management.
fn grant(binary: &Path, data_home: &Path) -> Result<(), Error> {
    let entry = DesktopEntry::new(DESKTOP_ID, "deskrinch window-management tests", binary)?;
    entry.install(data_home).map_err(|source| Error::Io {
        what: format!("install {}", entry.file_name()),
        source,
    })?;
    Ok(())
}
