//! `cargo xtask compositor-tests`: runs the crates' compositor tests inside
//! nested, headless compositors.
//!
//! The suites are [`suites::SUITES`]: each test binary, the compositors it
//! runs under and the passes it runs in. It builds the binaries, then for
//! each compositor and each [`suites::Pass`] (outputs at scale 1, at scale
//! 2, turned a quarter, flipped and turned) starts an isolated [`Session`]
//! and runs the suites meant for both, one binary after another.
//!
//! Under KWin each binary that needs a restricted interface is granted it
//! the way an installed deskrinch is: one desktop file per binary, in the
//! session's data directory, built from the interface constants of the
//! crates that binary uses (KWin's checks stay on). Each `kwin_not_granted`
//! binary checks that a binary without one is refused. The window-management
//! `session_end` tests end the session under a connected manager, so they
//! run last.

mod build;
pub(crate) mod renderers;
mod suites;

use std::path::{Path, PathBuf};

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_test_component::RENDERER_VARIABLE;

use self::build::Target;
use self::suites::{Pass, SUITES, Suite};
use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, CompositorRenderer, Session};

/// Runs the tests in each of `compositors` (all of them when empty), the
/// suites that render once per renderer `renderers` asks for.
pub(crate) fn run(
    root: &Path,
    compositors: &[Compositor],
    renderers: renderers::Choice,
) -> Result<Outcome, Error> {
    let plan = match renderers::Plan::for_machine(renderers) {
        Ok((plan, said)) => {
            println!("compositor-tests: {said}");
            plan
        }
        Err(why) => {
            eprintln!("compositor-tests: {why}");
            return Ok(Outcome::Failed);
        }
    };
    let compositors = if compositors.is_empty() {
        &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc][..]
    } else {
        compositors
    };
    let mut targets: Vec<Target> = Vec::new();
    for suite in &SUITES {
        if !targets.contains(&suite.target) {
            targets.push(suite.target);
        }
    }
    let built = build::test_binaries(root, &targets)?;
    let binary = |suite: &Suite| -> PathBuf {
        let index = targets
            .iter()
            .position(|target| *target == suite.target)
            .unwrap_or_default();
        built[index].clone()
    };
    let binaries: Vec<PathBuf> = SUITES.iter().map(binary).collect();
    let mut failed = Vec::new();
    for &compositor in compositors {
        for pass in Pass::ALL {
            let outputs = pass.outputs();
            println!("compositor-tests: {} with {outputs}", compositor.name());
            if !run_in(compositor, pass, &plan, &binaries)? {
                failed.push(format!("{} with {outputs}", compositor.name()));
            }
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

/// Runs the suites meant for `compositor` in `pass` in one fresh session;
/// `binaries` are [`SUITES`]' binaries. Returns whether they passed; on
/// failure, prints the compositor's log.
fn run_in(
    compositor: Compositor,
    pass: Pass,
    plan: &renderers::Plan,
    binaries: &[PathBuf],
) -> Result<bool, Error> {
    let session = Session::start(
        compositor,
        CompositorRenderer::Software,
        pass.outputs(),
        None,
        |data_home| match compositor {
            Compositor::Kwin => grant_all(binaries, data_home),
            Compositor::Sway | Compositor::Labwc => Ok(()),
        },
    )?;
    let mut passed = true;
    for (suite, binary) in SUITES.iter().zip(binaries) {
        if !suite.compositors.contains(&compositor) || !suite.passes.contains(&pass) {
            continue;
        }
        if !suite.renders {
            println!(
                "compositor-tests: {} {}",
                suite.target.package, suite.target.test
            );
            passed &= session.run_tests(binary, suite.filter, &[])?;
            continue;
        }
        for renderer in plan.under(compositor) {
            println!(
                "compositor-tests: {} {} with the {} renderer",
                suite.target.package,
                suite.target.test,
                renderer.env()
            );
            let env = [(RENDERER_VARIABLE, renderer.env())];
            passed &= session.run_tests(binary, suite.filter, &env)?;
        }
    }
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

/// Installs, for each suite KWin grants something, the one desktop file
/// that grants its binary all of it.
fn grant_all(binaries: &[PathBuf], data_home: &Path) -> Result<(), Error> {
    for (suite, binary) in SUITES.iter().zip(binaries) {
        let Some(grant) = &suite.grant else {
            continue;
        };
        let entry = DesktopEntry::new(grant.id, grant.name, binary, grant.wayland, grant.dbus)?;
        entry.install(data_home).map_err(|source| Error::Io {
            what: format!("install {}", entry.file_name()),
            source,
        })?;
    }
    Ok(())
}
