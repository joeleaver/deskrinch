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
//! binary checks that a binary without one is refused. A suite's programs (the
//! shell components its tests start that are not of its own crate) are
//! granted what the shell grants them, and handed to its tests by path
//! ([`deskrinch_test_component::programs`]). The window-management
//! `session_end` tests end the session under a connected manager, so they
//! run last.
//!
//! The components are built apart from the tests, by the one build of them
//! all ([`crate::components`]), whatever suites run, and the run keeps its
//! own copy of each ([`run_dir`]). Not with the tests: one invocation
//! unifies features over everything it builds, the test crates'
//! dev-dependencies included, and the suites would then start components
//! nobody builds for use. Not only those the chosen suites declare: a build
//! of some gives a component another unit than a build of all, and the unit
//! a suite starts would depend on which suites were chosen.
//!
//! A run can be narrowed to some suites, and to one test of one suite
//! ([`selection`]), and repeated in fresh sessions ([`repeat`]). A run that
//! tests rinch's software renderer alone uses no GPU, and tells its
//! sessions' clients so ([`Gpu`]): `cargo xtask desktop` inside then
//! composites in software, as on a machine without one.

mod args;
mod bounded;
mod build;
mod leaks;
pub(crate) mod renderers;
mod repeat;
mod run_dir;
pub(crate) mod selection;
mod suites;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_test_component::{RENDERER_VARIABLE, Renderer, programs};

use self::build::Target;
use self::run_dir::RunDir;
use self::selection::Selection;
use self::suites::{Pass, SUITES};
use crate::Outcome;
use crate::components::{self, Profile};
use crate::error::Error;
use crate::session::{Compositor, CompositorRenderer, Gpu, Session};

pub(crate) use self::args::Args;

/// Runs the tests as `args` asks.
pub(crate) fn run(root: &Path, args: &Args) -> Result<Outcome, Error> {
    match (args.repeat, &args.into) {
        (Some(times), Some(into)) => {
            repeat::run(args, times, into, Duration::from_secs(args.run_timeout))
        }
        _ => run_once(root, args),
    }
}

/// Runs the chosen suites once in each of the compositors asked for (all
/// of them when none), those that render once per renderer asked for.
fn run_once(root: &Path, args: &Args) -> Result<Outcome, Error> {
    let plan = match renderers::Plan::for_machine(args.renderer) {
        Ok((plan, said)) => {
            println!("compositor-tests: {said}");
            plan
        }
        Err(why) => {
            eprintln!("compositor-tests: {why}");
            return Ok(Outcome::Failed);
        }
    };
    let selection = Selection::new(&args.suites, args.test.as_deref())?;
    println!("compositor-tests: {}", selection.said());
    let compositors = if args.compositors.is_empty() {
        &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc][..]
    } else {
        &args.compositors
    };
    let sessions = sessions(compositors, &plan, &selection);
    if sessions.is_empty() {
        let names: Vec<_> = compositors.iter().map(|c| c.name()).collect();
        return Err(selection.nothing_runs(&names.join(", ")).into());
    }
    let built = build(root, &selection)?;
    if selection.test().is_some()
        && let Some(binary) = built.tests.values().next()
    {
        selection.check_test(&list_tests(binary)?)?;
    }
    let gpu = Gpu::from_allowed(plan.uses_gpu());
    let mut failed = Vec::new();
    for (place, runs) in sessions {
        let session = place.with_gpu(gpu);
        println!("compositor-tests: {}", session.said());
        if !run_in(session, &runs, &selection, &built)? {
            failed.push(session.said());
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

/// What a run built: the chosen suites' test binaries, by their rows in
/// [`SUITES`], and every component, by package, kept in the run's own
/// directory for as long as the run lasts.
struct Built {
    tests: BTreeMap<usize, PathBuf>,
    programs: BTreeMap<&'static str, PathBuf>,
    _kept: RunDir,
}

/// Builds the chosen suites' test binaries, then every component
/// ([`crate::components::build`]), kept in the run's own directory.
fn build(root: &Path, selection: &Selection) -> Result<Built, Error> {
    let mut targets: Vec<Target> = Vec::new();
    for &row in selection.rows() {
        if !targets.contains(&SUITES[row].target) {
            targets.push(SUITES[row].target);
        }
    }
    let built = build::test_binaries(root, &targets)?;
    let tests = selection
        .rows()
        .iter()
        .map(|&row| {
            let at = targets
                .iter()
                .position(|target| *target == SUITES[row].target);
            (row, built[at.unwrap_or_default()].clone())
        })
        .collect();
    let built = components::build(root, Profile::Dev)?;
    let keeping = |source| Error::Io {
        what: "keep the components for the run".into(),
        source,
    };
    let any = built
        .values()
        .next()
        .ok_or(components::BuildError::Missing {
            package: deskrinch_shell::COMPONENTS[0].package,
        })?;
    let kept = RunDir::beside(any).map_err(keeping)?;
    let programs = built
        .iter()
        .map(|(&package, binary)| Ok((package, kept.keep(binary).map_err(keeping)?)))
        .collect::<Result<_, Error>>()?;
    Ok(Built {
        tests,
        programs,
        _kept: kept,
    })
}

/// The compositor tests `binary` has, as it lists them.
fn list_tests(binary: &Path) -> Result<Vec<String>, Error> {
    let output = Command::new(binary)
        .args(["--list", "--ignored", "--format", "terse"])
        .output()
        .map_err(|source| Error::Spawn {
            program: binary.display().to_string(),
            source,
        })?;
    Ok(selection::listed(&String::from_utf8_lossy(&output.stdout)))
}

/// One session of a run: its compositor, what that renders with, its
/// pass, and whether its clients may use the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    compositor: Compositor,
    rendering: CompositorRenderer,
    pass: Pass,
    gpu: Gpu,
}

impl Place {
    /// The same session, its clients told `gpu`.
    fn with_gpu(self, gpu: Gpu) -> Self {
        Self { gpu, ..self }
    }

    /// What the run calls it.
    fn said(self) -> String {
        format!(
            "{} with {}{}",
            self.compositor.name(),
            self.pass.outputs(),
            match (self.compositor, self.rendering) {
                (Compositor::Sway | Compositor::Labwc, CompositorRenderer::Gpu) =>
                    ", rendering on the GPU",
                _ => "",
            }
        )
    }
}

/// One suite's run in a session: its row in [`SUITES`], and the renderer of
/// a rendering suite (`None` for one that does not render).
type Run = (usize, Option<Renderer>);

/// The sessions a run starts, in order, each with what runs in it; those
/// in which nothing runs are left out.
fn sessions(
    compositors: &[Compositor],
    plan: &renderers::Plan,
    selection: &Selection,
) -> Vec<(Place, Vec<Run>)> {
    let mut sessions = Vec::new();
    for &compositor in compositors {
        for pass in Pass::ALL {
            for rendering in [CompositorRenderer::Software, CompositorRenderer::Gpu] {
                let place = Place {
                    compositor,
                    rendering,
                    pass,
                    gpu: Gpu::Machine,
                };
                let runs = runs(place, plan, selection);
                if !runs.is_empty() {
                    sessions.push((place, runs));
                }
            }
        }
    }
    sessions
}

/// What runs at `place`: of the chosen suites that run the chosen test (if
/// any), those that do not render, in the software session only; each
/// rendering suite's renderers whose session that is.
fn runs(place: Place, plan: &renderers::Plan, selection: &Selection) -> Vec<Run> {
    let mut runs = Vec::new();
    for &row in selection.rows() {
        let suite = &SUITES[row];
        if !suite.compositors.contains(&place.compositor)
            || !suite.passes.contains(&place.pass)
            || selection.args(suite).is_none()
        {
            continue;
        }
        if !suite.renders {
            if place.rendering == CompositorRenderer::Software {
                runs.push((row, None));
            }
            continue;
        }
        for renderer in plan.renderers() {
            let gpu_client = renderer == Renderer::Gpu;
            if CompositorRenderer::for_client(place.compositor, gpu_client) == place.rendering {
                runs.push((row, Some(renderer)));
            }
        }
    }
    runs
}

/// Runs `runs` in one fresh session at `place`, each suite with the
/// arguments `selection` gives it and the paths of the programs it
/// declares; `built` is what the run built. Returns whether they passed; on
/// failure, prints the compositor's log.
fn run_in(place: Place, runs: &[Run], selection: &Selection, built: &Built) -> Result<bool, Error> {
    let compositor = place.compositor;
    let mut session = Session::start(
        compositor,
        place.rendering,
        place.pass.outputs(),
        None,
        place.gpu,
        |data_home| match compositor {
            Compositor::Kwin => grant_all(built, data_home),
            Compositor::Sway | Compositor::Labwc => Ok(()),
        },
    )?;
    let start = session.output_layout()?;
    let mut passed = true;
    for &(row, renderer) in runs {
        let (suite, binary) = (&SUITES[row], &built.tests[&row]);
        let args = selection.args(suite).unwrap_or_default();
        let mut env = programs::environment(
            suite
                .programs
                .iter()
                .map(|program| (program.package, built.programs[program.package].as_path())),
        );
        match renderer {
            None => println!("compositor-tests: {}", suite.target.name()),
            Some(renderer) => {
                println!(
                    "compositor-tests: {} with the {} renderer",
                    suite.target.name(),
                    renderer.env()
                );
                env.push((RENDERER_VARIABLE.to_owned(), renderer.env().into()));
            }
        }
        passed &= session.run_tests(binary, &args, &env)?;
        // A suite that ends the session leaves no outputs to list.
        if suite.ends_the_session() {
            continue;
        }
        // A failure to list them is the session's, at this suite, not the
        // run's: the other sessions still run.
        let now = session.output_layout();
        if let Some(said) = leaks::after_suite(&suite.target.name(), &start, now) {
            // What runs after it would run on outputs it does not expect,
            // or on a compositor that is gone.
            eprintln!("compositor-tests: {said}");
            passed = false;
            break;
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

/// Installs, for each chosen suite KWin grants something, the one desktop
/// file that grants its binary all of it; and for each program they
/// declare, what the shell grants that component ([`crate::grant`]).
fn grant_all(built: &Built, data_home: &Path) -> Result<(), Error> {
    for (&row, binary) in &built.tests {
        let Some(grant) = &SUITES[row].grant else {
            continue;
        };
        let entry = DesktopEntry::new(grant.id, grant.name, binary, grant.wayland, grant.dbus)?;
        entry.install(data_home).map_err(|source| Error::Io {
            what: format!("install {}", entry.file_name()),
            source,
        })?;
    }
    for &row in built.tests.keys() {
        for program in SUITES[row].programs {
            crate::grant::install(program, &built.programs[program.package], data_home)?;
        }
    }
    Ok(())
}
