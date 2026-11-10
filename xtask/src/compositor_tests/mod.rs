//! `cargo xtask compositor-tests`: runs the window-management crate's
//! compositor tests inside nested, headless compositors.
//!
//! For each compositor it builds the crate's test binaries, starts an
//! isolated [`Session`], and runs the ignored tests there. Under KWin the
//! `compositor` binary is granted the window-management protocol the way an
//! installed deskrinch is, by a desktop file in the session's data directory
//! (KWin's check stays on), and `kwin_not_granted` checks that a binary
//! without one is refused.

mod build;
mod session;

use std::path::Path;

use deskrinch_window_management::kwin_access::DesktopEntry;

use self::build::TestBinaries;
use self::session::Session;
use crate::Outcome;
use crate::error::Error;

/// A compositor the tests can run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Compositor {
    /// KWin, `kwin_wayland --virtual`: `org_kde_plasma_window_management`.
    Kwin,
    /// sway, on the wlroots headless backend:
    /// `zwlr_foreign_toplevel_management_v1`.
    Sway,
}

impl Compositor {
    /// Its name, as the tests read it from `DESKRINCH_TEST_COMPOSITOR`.
    fn name(self) -> &'static str {
        match self {
            Self::Kwin => "kwin",
            Self::Sway => "sway",
        }
    }
}

/// The desktop file id the `compositor` test binary is granted under.
const DESKTOP_ID: &str = "org.deskrinch.WindowManagementTests";

/// Runs the tests in each of `compositors` (all of them when empty).
pub(crate) fn run(root: &Path, compositors: &[Compositor]) -> Result<Outcome, Error> {
    let compositors = if compositors.is_empty() {
        &[Compositor::Kwin, Compositor::Sway][..]
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
    let session = Session::start(compositor, |data_home| match compositor {
        Compositor::Kwin => grant(&binaries.compositor, data_home),
        Compositor::Sway => Ok(()),
    })?;
    let mut passed = session.run_tests(&binaries.compositor)?;
    if compositor == Compositor::Kwin {
        passed &= session.run_tests(&binaries.kwin_not_granted)?;
    }
    if !passed {
        eprintln!(
            "compositor-tests: {} log:\n{}",
            compositor.name(),
            session.log()
        );
    }
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
