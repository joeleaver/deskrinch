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
//! shell components its tests start) are granted what the shell grants
//! them, and handed to its tests by path, the only way they start one
//! ([`deskrinch_test_component::programs`]; kept by [`component_rule`]).
//! The window-management `session_end` tests end the session under a
//! connected manager, so they run last.
//!
//! When a chosen suite declares a program, the components are built apart
//! from the tests, all of them, by the one build of them
//! ([`crate::components`]), and the run keeps its own copy of each
//! ([`run_dir`]); a run whose chosen suites declare none builds none
//! ([`built`]). Not with the tests: one invocation unifies
//! features over everything it builds, the test crates' dev-dependencies
//! included, and the suites would then start components nobody builds for
//! use. All of them once any: a build of some gives a component another unit
//! than a build of all, and the unit a suite starts would depend on which
//! suites were chosen. None for a run that starts none: it would build what
//! it never starts, and fail if one of them did not build.
//!
//! A run can be narrowed to some suites, and to one test of one suite
//! ([`selection`]), and repeated in fresh sessions ([`repeat`]). A run that
//! tests rinch's software renderer alone uses no GPU, and tells its
//! sessions' clients so ([`Gpu`]): `cargo xtask desktop` inside then
//! composites in software, as on a machine without one.
//!
//! CI builds once and runs in shards: `cargo xtask bundle build` builds
//! everything every suite runs into one bundle ([`bundle`]), and each shard
//! job runs `compositor-tests --prebuilt --shard NAME`, one compositor's
//! share of the suites ([`shards`]), from that bundle, building nothing.
//! Without those flags a run builds what it runs, as a developer's does.

mod args;
mod bounded;
mod build;
mod built;
pub(crate) mod bundle;
mod component_rule;
mod granted;
mod ignored_rule;
mod leaks;
mod passes;
pub(crate) mod renderers;
mod repeat;
mod run_dir;
pub(crate) mod selection;
pub(crate) mod shards;
mod suite;
mod suites;
mod timing;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use deskrinch_test_component::{RENDERER_VARIABLE, Renderer, programs};

use self::built::Built;
use self::selection::Selection;
use self::shards::Shard;
use self::suites::{Pass, SUITES};
use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, CompositorRenderer, Gpu, OutputSize, Outputs, Session};

pub(crate) use self::args::Args;
pub(crate) use self::build::{Backend, Target, test_binaries};
pub(crate) use self::bundle::Bundle;

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
    let shard = args.shard.as_deref().map(Shard::named).transpose()?;
    let selection = match shard {
        Some(shard) => Selection::of_shard(shard),
        None => Selection::new(&args.suites, args.test.as_deref())?,
    };
    println!("compositor-tests: {}", selection.said());
    let compositors = match shard {
        Some(shard) => &[shard.compositor][..],
        None if args.compositors.is_empty() => {
            &[Compositor::Kwin, Compositor::Sway, Compositor::Labwc][..]
        }
        None => &args.compositors,
    };
    let sessions = sessions(compositors, &args.sizes.sizes, &plan, &selection)?;
    if sessions.is_empty() {
        let names: Vec<_> = compositors.iter().map(|c| c.name()).collect();
        return Err(selection.nothing_runs(&names.join(", ")).into());
    }
    let built = if args.prebuilt {
        Built::from_bundle(&Bundle::open(root)?, &selection)?
    } else {
        Built::build(root, &selection)?
    };
    if selection.test().is_some()
        && let Some(binary) = built.tests.values().next()
    {
        selection.check_test(&list_tests(binary)?)?;
    }
    let gpu = Gpu::from_allowed(plan.uses_gpu());
    let mut failed = Vec::new();
    let mut timed = Vec::new();
    for (place, runs) in sessions {
        let session = place.with_gpu(gpu);
        println!("compositor-tests: {}", session.said());
        if !run_in(session, &runs, &selection, &built, &mut timed)? {
            failed.push(session.said());
        }
    }
    print!("{}", timing::summary(&timed));
    if failed.is_empty() {
        println!("compositor-tests: passed");
        Ok(Outcome::Passed)
    } else {
        eprintln!("compositor-tests: failed in {}", failed.join(", "));
        Ok(Outcome::Failed)
    }
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
#[derive(Debug, Clone, Copy, PartialEq)]
struct Place {
    compositor: Compositor,
    rendering: CompositorRenderer,
    pass: Pass,
    /// The pass's outputs, of the sizes asked for.
    outputs: Outputs,
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
            self.outputs,
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

/// The sessions a run starts, in order, each with what runs in it, its
/// outputs `sizes` large; those in which nothing runs are left out.
fn sessions(
    compositors: &[Compositor],
    sizes: &[OutputSize],
    plan: &renderers::Plan,
    selection: &Selection,
) -> Result<Vec<(Place, Vec<Run>)>, Error> {
    let mut sessions = Vec::new();
    for &compositor in compositors {
        for pass in Pass::EVERY {
            let outputs = if pass.has_outputs_of_its_own() {
                pass.outputs()
            } else {
                pass.outputs().sized(sizes)?
            };
            for rendering in [CompositorRenderer::Software, CompositorRenderer::Gpu] {
                let place = Place {
                    compositor,
                    rendering,
                    pass,
                    outputs,
                    gpu: Gpu::Machine,
                };
                let runs = runs(place, plan, selection);
                if runs.is_empty() {
                    continue;
                }
                if pass.has_outputs_of_its_own() && !sizes.is_empty() {
                    return Err(Error::OutputsOfItsOwn {
                        outputs: outputs.to_string(),
                    });
                }
                sessions.push((place, runs));
            }
        }
    }
    Ok(sessions)
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
/// declares; `built` is what the run built. Each suite run's time is added
/// to `timed`. Returns whether they passed; on failure, prints the
/// compositor's log.
fn run_in(
    place: Place,
    runs: &[Run],
    selection: &Selection,
    built: &Built,
    timed: &mut Vec<timing::SuiteRun>,
) -> Result<bool, Error> {
    let compositor = place.compositor;
    let mut session = Session::start(
        compositor,
        place.rendering,
        place.outputs,
        None,
        place.gpu,
        |data_home| match compositor {
            Compositor::Kwin => built.grant_all(data_home),
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
        let name = match renderer {
            None => suite.target.name(),
            Some(renderer) => {
                env.push((RENDERER_VARIABLE.to_owned(), renderer.env().into()));
                format!(
                    "{} with the {} renderer",
                    suite.target.name(),
                    renderer.env()
                )
            }
        };
        println!("compositor-tests: {name}");
        let mut clock = timing::Timer::start();
        passed &= session.run_tests(binary, &args, &env, &mut |at, line| {
            println!("{line}");
            clock.line(at, line);
        })?;
        timed.push(clock.finish(format!("{name}, {}", place.said())));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A pass with outputs of its own runs on them whatever the run asks,
    /// and a run that asks other sizes for such a session is refused; a run
    /// whose suites do not go there takes the sizes for its own passes.
    #[test]
    fn sizes_asked_for_never_change_a_pass_with_outputs_of_its_own() {
        let (plan, _) = renderers::Plan::new(
            renderers::Choice::Software,
            &crate::session::RenderNodes::of_machine(),
        )
        .unwrap();
        let size: OutputSize = "2560x1440".parse().unwrap();
        let client =
            Selection::new(&["deskrinch-test-client compositor".to_owned()], None).unwrap();
        let planned = sessions(&[Compositor::Kwin], &[], &plan, &client).unwrap();
        let own = planned
            .iter()
            .find(|(place, _)| place.pass == Pass::UhdBesideFullHd)
            .expect("the test client's row on the pass with outputs of its own");
        assert_eq!(own.0.outputs, Pass::UhdBesideFullHd.outputs());
        let refused = sessions(&[Compositor::Kwin], &[size], &plan, &client).unwrap_err();
        assert!(
            refused.to_string().contains("--output-size cannot change"),
            "{refused}"
        );
        // Any binary under KWin none of whose rows is on a pass with outputs
        // of its own, whichever that is as the table grows.
        let free = SUITES
            .iter()
            .map(|suite| suite.target)
            .find(|&target| {
                let rows = || SUITES.iter().filter(move |suite| suite.target == target);
                rows().any(|suite| suite.compositors.contains(&Compositor::Kwin))
                    && rows()
                        .flat_map(|suite| suite.passes)
                        .all(|pass| !pass.has_outputs_of_its_own())
            })
            .expect("a binary under KWin with no pass of outputs of its own");
        let free = Selection::new(&[free.name()], None).unwrap();
        let sized = sessions(&[Compositor::Kwin], &[size], &plan, &free).unwrap();
        assert_ne!(sized.len(), 0, "no session for {free:?}");
        assert!(sized.iter().all(|(place, _)| place.outputs.size(0) == size));
    }
}
