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
//!   frames, and CPU per frame; each run notes which threads woke most.
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
mod granting;
mod output;
mod probe;
mod procfs;
mod renderer;
mod report;
mod show;
mod smaps;
mod state;
mod system;
mod window;

pub(crate) use renderer::{Expected, Renderer};
pub(crate) use show::Summons;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use self::granting::Granting;
use self::renderer::check_renderer;
use self::window::Stimulus;
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
    second, frames, and CPU per frame; and, in each run's notes, the
    threads that woke most in it, by name;
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

--window APP_ID opens one window with that app id in the session at the
idle window's start (once it has had its first configure and drawn), and
keeps it open to its end: the idle figures then include what it costs the
program, and the report also prints the idle window's wake-ups and frames
as totals. A run with it minus one without is the cost of a window
appearing plus what a window held open costs over the window. A run fails
if the window cannot be opened or is gone by the end.

Under KWin a program that is one of the shell's components (by its file
name, or as --component PACKAGE names it) is granted what deskrinch-shell
grants that component, as the desktop grants it; anything else is granted
nothing. Each run's notes say what was granted.

Each run keeps the program's whole output in a file its notes name. A run
whose program logged an error fails, printing the lines: a program that
says it is broken is not measured. --allow-errors measures it anyway, with
the lines in the notes.

--state DIR starts each run's state directory (XDG_STATE_HOME) with a copy
of DIR's contents, such as the launcher's record of starts
(deskrinch/starts.toml); the report says the state was seeded, and from
where. A DIR that is not a readable directory fails before any session.

Needs valgrind and callgrind_control for --instructions.";

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
    /// With `--state`: what each run's state directory starts with.
    pub(crate) state: Option<PathBuf>,
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    /// With `--summon`: the program starts hidden, and each show is measured
    /// instead of the start.
    pub(crate) summons: Option<Summons>,
    /// With `--window`: the app id of a window opened at the idle window's
    /// start and kept open to its end.
    pub(crate) window: Option<String>,
    /// `--allow-errors`: measure a program that logged errors, with the
    /// lines in the run's notes, instead of failing the run.
    pub(crate) allow_errors: bool,
    /// With `--component`: the shell component the program is, when its
    /// file name is not that package's (a copied binary); under KWin it is
    /// granted what `deskrinch-shell` grants that component.
    pub(crate) component: Option<String>,
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
    if let Some(state) = &request.state {
        state::check(state)?;
    }
    let program = request.program.to_string_lossy().into_owned();
    let granting = Granting::of(request)?;
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
        in_session(request, granting.as_ref(), |session| {
            let command = client(session, request, Some(dir));
            match &request.summons {
                Some(summons) => {
                    let ask = |args: &[OsString]| asked(session, request, Some(dir), args);
                    show::measure(command, &ask, &program, timing, summons).map(drop)
                }
                None => probe::measure(command, &program, timing, &Stimulus::none()).map(drop),
            }
        })?;
    }
    let timing = probe::Timing {
        first_frame: request.first_frame_timeout,
        settle: request.settle,
        idle: request.idle,
    };
    let kept = output::Kept::new()?;
    let mut runs = Vec::new();
    for index in 1..=request.runs {
        eprintln!("measure: run {index} of {}", request.runs);
        let mut run = in_session(request, granting.as_ref(), |session| {
            measure_run(request, session, index, cache, &program, timing)
        })?;
        output::settle(&mut run, &kept.file(index), &program, request.allow_errors)?;
        run.note("granted", Granting::note(granting.as_ref()));
        runs.push(run);
    }
    println!("{}", header(request));
    if let Some(note) = state::note(request.state.as_deref()) {
        println!("{note}");
    }
    println!("{}", report::render(&program, &runs));
    match request.expect_renderer {
        Some(expected) => Ok(check_renderer(expected, &runs)),
        None => Ok(Outcome::Passed),
    }
}

/// Measures run `index` of `program` in `session` as `request` asks: under
/// Callgrind with `--instructions`, else from `/proc`; its shows with
/// `--summon`, else its start and idle window.
fn measure_run(
    request: &Request,
    session: &Session,
    index: u32,
    cache: Option<&Path>,
    program: &str,
    timing: probe::Timing,
) -> Result<report::Run, Error> {
    let ask = |args: &[OsString]| asked(session, request, cache, args);
    let stimulus = || Stimulus::of(session, request.window.as_deref());
    if !request.instructions {
        let command = client(session, request, cache);
        return match &request.summons {
            Some(summons) => show::measure(command, &ask, program, timing, summons),
            None => probe::measure(command, program, timing, &stimulus()),
        };
    }
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
            show::measure_instructions(command, &control, &ask, program, &out, timing, summons)
        }
        None => callgrind::measure(command, control, program, &out, timing, &stimulus()),
    }
}

/// Runs `measure` in a fresh session, which is ended afterwards whatever
/// the outcome.
fn in_session<T>(
    request: &Request,
    granting: Option<&Granting>,
    measure: impl FnOnce(&Session) -> Result<T, Error>,
) -> Result<T, Error> {
    let session = Session::start(
        request.compositor,
        request.compositor_renderer(),
        Outputs::plain(1),
        None,
        |data_home| granting.map_or(Ok(()), |granting| granting.install(data_home)),
    )?;
    let result = match &request.state {
        Some(state) => state::seed(state, &session.root().join("state")),
        None => Ok(()),
    }
    .and_then(|()| measure(&session));
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
