//! `cargo xtask measure`: what a shell component costs, measured in
//! isolated sessions.
//!
//! Each run starts the program, a rinch binary, as the only client of a
//! fresh [`Session`] and reports:
//!
//! - **Start-up**: the CPU time the process used up to its first presented
//!   frame, in all and per thread (threads that ended before it, such as
//!   shader-compiling workers, included).
//! - **Memory** once settled: resident and proportional sizes, the resident
//!   set by what is mapped ([`smaps`]), and GPU memory, which is not in the
//!   resident set ([`gpu`]).
//! - **Idle**: over a window, CPU time, wake-ups and preemptions per second,
//!   frames, and CPU per frame.
//! - The renderer that presented: rinch's software renderer, or the GPU
//!   renderer on the adapter wgpu chose, flagged when that is a software
//!   driver.
//!
//! With `--instructions` the program runs under Callgrind instead and the
//! report is instruction counts ([`callgrind`]): deterministic, and so the
//! figure to compare on a loaded machine. CPU time is the next best thing.
//! Wall time is reported only beside the load average, apart from the rest,
//! as not comparable.
//!
//! With `--summon` the program starts hidden and the shows are measured
//! instead ([`show`]): from the command that asks it to show (the program
//! run again with the summon arguments) to the first frame after it, the
//! first show cold and each later one after a hide.
//!
//! The first frame is rinch's: the program is run with `RINCH_PERF=1`,
//! which makes rinch write a line after every presented frame.

mod callgrind;
mod gpu;
mod probe;
mod procfs;
mod report;
mod show;
mod smaps;
mod system;

pub(crate) use show::Summons;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, CompositorRenderer, Outputs, RenderNodes, Session};

/// `cargo xtask measure --help`: what is measured, and how.
pub(crate) const LONG_ABOUT: &str = "\
Measure what a rinch program (a shell component) costs, in isolated sessions.

Each run starts the program as the only client of a fresh nested compositor
(the session `cargo xtask session` describes) with RINCH_PERF=1, and reports:
  - CPU time (user+system) up to its first presented frame, in all and per
    thread, threads that ended before it included;
  - memory once settled: Rss, Pss, anonymous, file, shmem and swap; the
    resident set by what is mapped (the Vulkan driver, LLVM, fonts, GTK, the
    program's binary, the heap); and GPU memory from the DRM fdinfo, which is
    not in Rss;
  - over an idle window: CPU time per minute, wake-ups and preemptions per
    second, frames, and CPU per frame;
  - the renderer that presented: rinch's software renderer, or the GPU
    renderer on the adapter wgpu chose, marked when that is a software
    Vulkan driver.

--instructions runs the program under Valgrind's Callgrind and reports
instructions to the first frame, in the idle window, and per frame: the
deterministic figure, for a loaded machine. Without it, CPU time is the
figure to compare; wall time is printed apart, beside the load average, and
is not comparable.

Each session starts with an empty shader cache (--shader-cache cold);
--shader-cache warm fills one with an unmeasured start first and keeps it,
outside the sessions, for every run.

Under sway and labwc the compositor renders with pixman and offers no dmabuf,
so wgpu would offer only a software Vulkan driver (lavapipe), which rinch's
default renderer declines for its software renderer. --renderer gpu under
sway or labwc therefore gets a session whose compositor renders on the GPU
(wlroots' GLES2 renderer on the GPU's render node), and fails where the
machine has none. KWin composites with the GPU either way.

--summon ARGS --dismiss ARGS measures showing instead of starting: the
program starts hidden, settles, and is shown --shows times by running it
again with the summon arguments, hidden again with the dismiss arguments
after each show. Each show reports CPU time (or with --instructions the
instructions) from the summon command to the first frame after it, the
first show cold, each later one after a hide. The summon command's own
process is not counted. Memory and the idle window are not measured.

Needs valgrind and callgrind_control for --instructions.";

/// Which renderer the program is told to use, through rinch's
/// `RINCH_RENDERER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Renderer {
    /// rinch's choice (the GPU, falling back to software).
    Auto,
    /// The GPU renderer (Vello on wgpu).
    Gpu,
    /// The software renderer (tiny-skia).
    Software,
}

impl Renderer {
    fn env(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::Software => "software",
        }
    }
}

/// The renderer `--expect-renderer` requires to have presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Expected {
    /// rinch's software renderer.
    Software,
    /// rinch's GPU renderer, on a GPU (not a software Vulkan driver).
    Gpu,
}

impl Expected {
    fn renderer(self) -> deskrinch_test_component::Renderer {
        match self {
            Self::Software => deskrinch_test_component::Renderer::Software,
            Self::Gpu => deskrinch_test_component::Renderer::Gpu,
        }
    }
}

/// Whether every run presented with `expected`; says which did not, and
/// with what.
fn check_renderer(expected: Expected, runs: &[report::Run]) -> Outcome {
    let wrong: Vec<String> = runs
        .iter()
        .enumerate()
        .filter(|(_, run)| {
            !run.presented
                .as_ref()
                .is_some_and(|presented| expected.renderer().presented(presented))
        })
        .map(|(index, run)| {
            format!(
                "run {}: {}",
                index + 1,
                presented_name(run.presented.as_ref())
            )
        })
        .collect();
    if wrong.is_empty() {
        println!("measure: every run presented with the {expected:?} renderer, as expected");
        Outcome::Passed
    } else {
        eprintln!(
            "measure: expected the {expected:?} renderer to present, but {}",
            wrong.join("; ")
        );
        Outcome::Failed
    }
}

/// What a run's program said presented.
fn presented_name(presented: Option<&deskrinch_test_component::Presented>) -> String {
    use deskrinch_test_component::Presented;
    match presented {
        None => "it did not say which renderer presented".to_owned(),
        Some(Presented::Software) => "rinch's software renderer presented".to_owned(),
        Some(Presented::Gpu(adapter)) if adapter.is_software() => format!(
            "the GPU renderer presented on {}, a software Vulkan driver",
            adapter.name
        ),
        Some(Presented::Gpu(adapter)) => {
            format!("the GPU renderer presented on {}", adapter.name)
        }
    }
}

/// The state of the GPU driver's shader cache at each start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ShaderCache {
    /// Empty: each session's cache is private and new, as on a first start.
    Cold,
    /// Filled by one unmeasured start beforehand, as on every later start
    /// in a real session (`MESA_SHADER_CACHE_DIR`, kept across the runs).
    Warm,
}

/// What `cargo xtask measure` was asked to do.
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) compositor: Compositor,
    pub(crate) runs: u32,
    pub(crate) renderer: Option<Renderer>,
    /// With `--expect-renderer`: the renderer every run must have presented
    /// with, or the task fails naming what did.
    pub(crate) expect_renderer: Option<Expected>,
    pub(crate) shader_cache: ShaderCache,
    pub(crate) settle: Duration,
    pub(crate) idle: Duration,
    pub(crate) first_frame_timeout: Duration,
    pub(crate) instructions: bool,
    /// Where to keep each run's Callgrind profile (`run-N/`), for
    /// `callgrind_annotate`; inside the session, and gone with it, if unset.
    pub(crate) profiles: Option<PathBuf>,
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    /// With `--summon`: the program starts hidden, and each show is measured
    /// instead of the start.
    pub(crate) summons: Option<Summons>,
}

impl Request {
    /// What each run's compositor renders with: the GPU when rinch's GPU
    /// renderer is asked for under sway or labwc, the only session it can
    /// present on there ([`CompositorRenderer::for_client`]).
    fn compositor_renderer(&self) -> CompositorRenderer {
        CompositorRenderer::for_client(self.compositor, self.renderer == Some(Renderer::Gpu))
    }
}

/// Runs the measurement and prints its report.
pub(crate) fn run(request: &Request) -> Result<Outcome, Error> {
    let program = request.program.to_string_lossy().into_owned();
    // A warm cache lives outside the sessions, for as long as this task.
    let cache_dir = (request.shader_cache == ShaderCache::Warm)
        .then(|| {
            tempfile::Builder::new()
                .prefix("deskrinch-measure-cache-")
                .tempdir()
                .map_err(|source| Error::Io {
                    what: "create the shader cache directory".into(),
                    source,
                })
        })
        .transpose()?;
    let cache = cache_dir.as_ref().map(tempfile::TempDir::path);
    if let Some(dir) = cache {
        eprintln!("measure: filling the shader cache with one unmeasured start");
        let timing = probe::Timing {
            first_frame: request.first_frame_timeout,
            settle: Duration::ZERO,
            idle: Duration::ZERO,
        };
        in_session(request, |session| {
            let command = client(session, request, Some(dir));
            match &request.summons {
                Some(summons) => {
                    let ask = |args: &[OsString]| asked(session, request, Some(dir), args);
                    show::measure(command, &ask, &program, timing, summons).map(drop)
                }
                None => probe::measure(command, &program, timing).map(drop),
            }
        })?;
    }
    let timing = probe::Timing {
        first_frame: request.first_frame_timeout,
        settle: request.settle,
        idle: request.idle,
    };
    let mut runs = Vec::new();
    for index in 1..=request.runs {
        eprintln!("measure: run {index} of {}", request.runs);
        let run = in_session(request, |session| {
            if request.instructions {
                let dir = request.profiles.as_ref().map_or_else(
                    || session.root().join("callgrind"),
                    |dir| dir.join(format!("run-{index}")),
                );
                std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
                    what: format!("create {}", dir.display()),
                    source,
                })?;
                let out = dir.join("callgrind.out");
                let mut command = session.client("valgrind".as_ref());
                command
                    .args(callgrind::valgrind_args(&out))
                    .arg(&request.program);
                environment(&mut command, request, cache);
                command.args(&request.args);
                let control = || session.client("callgrind_control".as_ref());
                match &request.summons {
                    Some(summons) => {
                        let ask = |args: &[OsString]| asked(session, request, cache, args);
                        show::measure_instructions(
                            command, &control, &ask, &program, &out, timing, summons,
                        )
                    }
                    None => callgrind::measure(command, control, &program, &out, timing),
                }
            } else {
                let command = client(session, request, cache);
                match &request.summons {
                    Some(summons) => {
                        let ask = |args: &[OsString]| asked(session, request, cache, args);
                        show::measure(command, &ask, &program, timing, summons)
                    }
                    None => probe::measure(command, &program, timing),
                }
            }
        })?;
        runs.push(run);
    }
    println!("{}", header(request));
    println!("{}", report::render(&program, &runs));
    match request.expect_renderer {
        Some(expected) => Ok(check_renderer(expected, &runs)),
        None => Ok(Outcome::Passed),
    }
}

/// Runs `measure` in a fresh session, which is ended afterwards whatever
/// the outcome.
fn in_session<T>(
    request: &Request,
    measure: impl FnOnce(&Session) -> Result<T, Error>,
) -> Result<T, Error> {
    let session = Session::start(
        request.compositor,
        request.compositor_renderer(),
        Outputs::plain(1),
        None,
        |_| Ok(()),
    )?;
    let result = measure(&session);
    let ended = session.end();
    let value = result?;
    ended?;
    Ok(value)
}

/// The measured program as a client of `session`.
fn client(session: &Session, request: &Request, cache: Option<&Path>) -> Command {
    let mut command = session.client(&request.program);
    environment(&mut command, request, cache);
    command.args(&request.args);
    command
}

/// The program run again with `args` (`--summon`'s or `--dismiss`'s) as a
/// client of `session`, in the measured program's environment, not under
/// Valgrind: it asks the measured process, and is not measured itself.
fn asked(session: &Session, request: &Request, cache: Option<&Path>, args: &[OsString]) -> Command {
    let mut command = session.client(&request.program);
    environment(&mut command, request, cache);
    command.args(args);
    command
}

/// The measured program's environment: rinch's frame lines, wgpu's
/// adapter line, the renderer and the shader cache asked for.
fn environment(command: &mut Command, request: &Request, cache: Option<&Path>) {
    let callers_log = std::env::var("RUST_LOG").ok();
    command
        .env("RINCH_PERF", "1")
        .env(
            "RUST_LOG",
            deskrinch_test_component::log::rust_log(callers_log.as_deref()),
        )
        .env("NO_COLOR", "1");
    if let Some(renderer) = request.renderer {
        command.env("RINCH_RENDERER", renderer.env());
    }
    match cache {
        Some(dir) => command.env("MESA_SHADER_CACHE_DIR", dir),
        // The session's own cache, empty: never one the caller set.
        None => command.env_remove("MESA_SHADER_CACHE_DIR"),
    };
}

/// What was measured, where and how.
fn header(request: &Request) -> String {
    let args: Vec<String> = request
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    format!(
        "Measured: {} {}\n\
         Compositor: {} (nested, headless; {})\n\
         Renderer: {}; shader cache: {}; {}\n\
         Machine: {}",
        request.program.to_string_lossy(),
        args.join(" "),
        request.compositor.name(),
        match (request.compositor, request.compositor_renderer()) {
            (Compositor::Kwin, _) => "it composites with the GPU through EGL".to_owned(),
            (_, CompositorRenderer::Gpu) => format!(
                "wlroots with the GLES2 renderer on {}, as its log says",
                RenderNodes::of_machine()
                    .gpu()
                    .map_or_else(|| "the GPU".to_owned(), |node| node.display().to_string())
            ),
            (_, CompositorRenderer::Software) => match request.renderer {
                Some(Renderer::Auto) | None => "wlroots with the pixman renderer, which offers no \
                     dmabuf; rinch's GPU renderer would have only a software Vulkan driver, which \
                     its default declines"
                    .to_owned(),
                Some(Renderer::Software | Renderer::Gpu) => {
                    "wlroots with the pixman renderer".to_owned()
                }
            },
        },
        request
            .renderer
            .map_or("as the caller's environment says", Renderer::env),
        match request.shader_cache {
            ShaderCache::Cold => "cold (empty)",
            ShaderCache::Warm => "warm (filled by an unmeasured start)",
        },
        match (&request.summons, request.instructions) {
            (Some(_), true) => "shows, under Callgrind: instruction counts only",
            (Some(_), false) => "shows: CPU from each summon to its first frame",
            (None, true) => "under Callgrind: instruction counts only",
            (None, false) => "settle and idle windows as asked",
        },
        system::describe(),
    )
}

#[cfg(test)]
mod tests {
    use deskrinch_test_component::Presented;
    use deskrinch_test_component::log::Adapter;

    use super::*;

    fn run(presented: Option<Presented>) -> report::Run {
        report::Run {
            presented,
            ..report::Run::default()
        }
    }

    fn gpu(device_type: &str) -> Presented {
        Presented::Gpu(Adapter {
            name: "llvmpipe".into(),
            driver: "llvmpipe".into(),
            driver_info: String::new(),
            device_type: device_type.into(),
            backend: "Vulkan".into(),
        })
    }

    #[test]
    fn every_run_must_present_with_the_renderer_expected() {
        let software = [
            run(Some(Presented::Software)),
            run(Some(Presented::Software)),
        ];
        assert_eq!(
            check_renderer(Expected::Software, &software),
            Outcome::Passed
        );
        assert_eq!(check_renderer(Expected::Gpu, &software), Outcome::Failed);
        let mixed = [run(Some(Presented::Software)), run(None)];
        assert_eq!(check_renderer(Expected::Software, &mixed), Outcome::Failed);
        // A software Vulkan driver is not the GPU.
        assert_eq!(
            check_renderer(Expected::Gpu, &[run(Some(gpu("Cpu")))]),
            Outcome::Failed
        );
        assert_eq!(
            check_renderer(Expected::Gpu, &[run(Some(gpu("DiscreteGpu")))]),
            Outcome::Passed
        );
    }

    #[test]
    fn says_what_presented_instead() {
        assert_eq!(
            presented_name(Some(&gpu("Cpu"))),
            "the GPU renderer presented on llvmpipe, a software Vulkan driver"
        );
        assert_eq!(
            presented_name(None),
            "it did not say which renderer presented"
        );
    }
}
