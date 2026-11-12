//! `cargo xtask compositor-tests`: runs the crates' compositor tests inside
//! nested, headless compositors.
//!
//! The suites are [`suites::SUITES`]: each test binary, the compositors it
//! runs under and the passes it runs in. It builds the binaries, then for
//! each compositor and each [`suites::Pass`] (outputs at scale 1, at scale
//! 2, turned a quarter, flipped and turned) starts an isolated [`Session`]
//! and runs the suites meant for both, one binary after another.
//!
//! Every session's compositor renders in software (sway and labwc on
//! pixman), except one more per compositor and pass where rinch's GPU
//! renderer is tested under sway or labwc: that session's compositor renders
//! on the GPU, the only kind a GPU client can present on there
//! ([`CompositorRenderer::for_client`]), and it runs only the rendering
//! suites' GPU pass.
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
use deskrinch_test_component::{RENDERER_VARIABLE, Renderer};

use self::build::{Bin, Target};
use self::suites::{Grant, Pass, Program, SUITES, Suite};
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
    let programs: Vec<&Program> = SUITES.iter().flat_map(|suite| suite.programs).collect();
    let bins: Vec<Bin> = programs.iter().map(|program| program.bin).collect();
    let built = build::test_binaries(root, &targets, &bins)?;
    let binary = |suite: &Suite| -> PathBuf {
        let index = targets
            .iter()
            .position(|target| *target == suite.target)
            .unwrap_or_default();
        built.tests[index].clone()
    };
    let binaries: Vec<PathBuf> = SUITES.iter().map(binary).collect();
    let granted: Vec<(&Grant, PathBuf)> = programs
        .iter()
        .map(|program| &program.grant)
        .zip(built.programs)
        .collect();
    let mut failed = Vec::new();
    for &compositor in compositors {
        for pass in Pass::ALL {
            for rendering in [CompositorRenderer::Software, CompositorRenderer::Gpu] {
                let runs = runs(compositor, rendering, pass, &plan);
                if runs.is_empty() {
                    continue;
                }
                let session = format!(
                    "{} with {}{}",
                    compositor.name(),
                    pass.outputs(),
                    match (compositor, rendering) {
                        (Compositor::Sway | Compositor::Labwc, CompositorRenderer::Gpu) =>
                            ", rendering on the GPU",
                        _ => "",
                    }
                );
                println!("compositor-tests: {session}");
                if !run_in(compositor, rendering, pass, &runs, &binaries, &granted)? {
                    failed.push(session);
                }
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

/// One suite's run in a session: [`SUITES`]' index, and the renderer of a
/// rendering suite (`None` for one that does not render).
type Run = (usize, Option<Renderer>);

/// What runs under `compositor` in `pass` in its session that renders with
/// `rendering`: the suites that do not render, in the software session
/// only; each rendering suite's renderers whose session that is.
fn runs(
    compositor: Compositor,
    rendering: CompositorRenderer,
    pass: Pass,
    plan: &renderers::Plan,
) -> Vec<Run> {
    let mut runs = Vec::new();
    for (index, suite) in SUITES.iter().enumerate() {
        if !suite.compositors.contains(&compositor) || !suite.passes.contains(&pass) {
            continue;
        }
        if !suite.renders {
            if rendering == CompositorRenderer::Software {
                runs.push((index, None));
            }
            continue;
        }
        for renderer in plan.renderers() {
            let gpu_client = renderer == Renderer::Gpu;
            if CompositorRenderer::for_client(compositor, gpu_client) == rendering {
                runs.push((index, Some(renderer)));
            }
        }
    }
    runs
}

/// Runs `runs` under `compositor` rendering with `rendering`, with `pass`'s
/// outputs, in one fresh session; `binaries` are [`SUITES`]' binaries, and
/// `programs` the programs they start that KWin grants something.
/// Returns whether they passed; on failure, prints the compositor's log.
fn run_in(
    compositor: Compositor,
    rendering: CompositorRenderer,
    pass: Pass,
    runs: &[Run],
    binaries: &[PathBuf],
    programs: &[(&Grant, PathBuf)],
) -> Result<bool, Error> {
    let session =
        Session::start(
            compositor,
            rendering,
            pass.outputs(),
            None,
            |data_home| match compositor {
                Compositor::Kwin => grant_all(binaries, programs, data_home),
                Compositor::Sway | Compositor::Labwc => Ok(()),
            },
        )?;
    let mut passed = true;
    for &(index, renderer) in runs {
        let (suite, binary) = (&SUITES[index], &binaries[index]);
        let Some(renderer) = renderer else {
            println!(
                "compositor-tests: {} {}",
                suite.target.package, suite.target.test
            );
            passed &= session.run_tests(binary, suite.filter, &[])?;
            continue;
        };
        println!(
            "compositor-tests: {} {} with the {} renderer",
            suite.target.package,
            suite.target.test,
            renderer.env()
        );
        let env = [(RENDERER_VARIABLE, renderer.env())];
        passed &= session.run_tests(binary, suite.filter, &env)?;
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
/// that grants its binary all of it, and one for each program the suites'
/// tests start.
fn grant_all(
    binaries: &[PathBuf],
    programs: &[(&Grant, PathBuf)],
    data_home: &Path,
) -> Result<(), Error> {
    let suites = SUITES
        .iter()
        .zip(binaries)
        .filter_map(|(suite, binary)| suite.grant.as_ref().map(|grant| (grant, binary)));
    let programs = programs.iter().map(|(grant, binary)| (*grant, binary));
    for (grant, binary) in suites.chain(programs) {
        let entry = DesktopEntry::new(grant.id, grant.name, binary, grant.wayland, grant.dbus)?;
        entry.install(data_home).map_err(|source| Error::Io {
            what: format!("install {}", entry.file_name()),
            source,
        })?;
    }
    Ok(())
}
