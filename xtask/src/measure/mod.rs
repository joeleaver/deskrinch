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
//! first show cold and each later one after a hide; and its memory once,
//! hidden and settled, before the first show.
//!
//! The first frame is rinch's: the program is run with `RINCH_PERF=1`,
//! which makes rinch write a line after every presented frame. It is the
//! main window's, or with `--windows N` the last of N windows' first frames
//! ([`windows`]).

mod args;
mod callgrind;
mod environment;
mod gpu;
mod granting;
mod help;
mod input;
mod output;
mod probe;
mod procfs;
mod renderer;
mod report;
mod settle;
mod show;
mod smaps;
mod state;
mod system;
mod window;
mod windows;

pub(crate) use args::Args;
pub(crate) use help::LONG_ABOUT;
pub(crate) use input::MeasuredInputError;
pub(crate) use windows::WindowsError;

use self::environment::parse_env;
use self::input::{Inputs, Point, Pointer, QueryLine, Typing, parse_text};
use self::renderer::{Expected, Renderer};
use self::show::Summons;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use self::environment::{environment, environment_set};
use self::granting::Grant;
use self::renderer::check_renderer;
use self::window::Stimulus;
use self::windows::Expect;
use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, CompositorRenderer, Gpu, Outputs, RenderNodes, Session};

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
    /// How many outputs each run's session has.
    pub(crate) outputs: u32,
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
    /// With `--windows`: how many windows the program opens, each read from
    /// rinch's per-window lines, instead of its main window.
    pub(crate) windows: Option<u32>,
    /// `--allow-errors`: measure a program that logged errors, with the
    /// lines in the run's notes, instead of failing the run.
    pub(crate) allow_errors: bool,
    /// With `--component`: the shell component the program is, when its
    /// file name is not that package's (a copied binary); under KWin it is
    /// granted what `deskrinch-shell` grants that component.
    pub(crate) component: Option<String>,
    /// With `--env`: variables set on the measured program, after its own.
    pub(crate) env: Vec<(String, String)>,
    /// With `--place-pointer`: the pointer placed, then moved a step at a
    /// time, after the idle window or after each show.
    pub(crate) pointer: Option<Pointer>,
}

impl Request {
    /// Whether it gives the program input: a query typed (`--type`), or
    /// the pointer moved.
    fn gives_input(&self) -> bool {
        self.pointer.is_some() || self.summons.as_ref().is_some_and(|s| s.typing.is_some())
    }

    /// What each run's compositor renders with: the GPU when rinch's GPU
    /// renderer is asked for under sway or labwc, the only session it can
    /// present on there ([`CompositorRenderer::for_client`]).
    fn compositor_renderer(&self) -> CompositorRenderer {
        CompositorRenderer::for_client(self.compositor, self.renderer == Some(Renderer::Gpu))
    }

    /// A request for `program` with every option at its default, for tests.
    #[cfg(test)]
    pub(crate) fn new(program: OsString) -> Self {
        Self {
            compositor: Compositor::Kwin,
            runs: 1,
            outputs: 1,
            renderer: None,
            expect_renderer: None,
            shader_cache: ShaderCache::Cold,
            settle: Duration::ZERO,
            idle: Duration::ZERO,
            first_frame_timeout: Duration::ZERO,
            instructions: false,
            profiles: None,
            state: None,
            program,
            args: Vec::new(),
            summons: None,
            window: None,
            windows: None,
            allow_errors: false,
            component: None,
            env: Vec::new(),
            pointer: None,
        }
    }
}

/// Runs the measurement and prints its report.
pub(crate) fn run(request: &Request) -> Result<Outcome, Error> {
    if let Some(state) = &request.state {
        state::check(state)?;
    }
    if let Some(pointer) = &request.pointer {
        pointer.check(request.outputs)?;
    }
    let program = request.program.to_string_lossy().into_owned();
    let grant = Grant::of(request)?;
    // Only a run that gives input depends on finding this binary, the
    // input client.
    let client = request.gives_input().then(input_executable).transpose()?;
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
        in_session(request, &grant, client.as_deref(), |session| {
            with_inputs(request, session, client.as_deref(), |inputs| {
                let command = program_client(session, request, Some(dir));
                match &request.summons {
                    Some(summons) => {
                        let ask = |args: &[OsString]| asked(session, request, Some(dir), args);
                        show::measure(command, &ask, inputs, &program, timing, summons).map(drop)
                    }
                    None => probe::measure(
                        command,
                        &program,
                        timing,
                        &Stimulus::none(),
                        Expect::of(request.windows),
                        inputs,
                    )
                    .map(drop),
                }
            })
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
        let mut run = in_session(request, &grant, client.as_deref(), |session| {
            with_inputs(request, session, client.as_deref(), |inputs| {
                measure_run(request, session, index, cache, &program, timing, inputs)
            })
        })?;
        output::settle(&mut run, &kept.file(index), &program, request.allow_errors)?;
        run.note("granted", grant.note());
        if let Some(set) = environment_set(request) {
            run.note("environment set", set);
        }
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
/// `--summon`, else its start and idle window; with `inputs`, the input
/// it gives.
fn measure_run(
    request: &Request,
    session: &Session,
    index: u32,
    cache: Option<&Path>,
    program: &str,
    timing: probe::Timing,
    inputs: Option<&Inputs<'_>>,
) -> Result<report::Run, Error> {
    let ask = |args: &[OsString]| asked(session, request, cache, args);
    let stimulus = || Stimulus::of(session, request.window.as_deref());
    let expect = Expect::of(request.windows);
    if !request.instructions {
        let command = program_client(session, request, cache);
        return match &request.summons {
            Some(summons) => show::measure(command, &ask, inputs, program, timing, summons),
            None => probe::measure(command, program, timing, &stimulus(), expect, inputs),
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
        Some(summons) => show::measure_instructions(
            command, &control, &ask, inputs, program, &out, timing, summons,
        ),
        None => callgrind::measure(
            command,
            control,
            program,
            &out,
            timing,
            &stimulus(),
            expect,
            inputs,
        ),
    }
}

/// Runs `measure` in a fresh session, which is ended afterwards whatever
/// the outcome. Under KWin the program is granted `grant`, and the input
/// client (run from `client`) the input KWin restricts.
fn in_session<T>(
    request: &Request,
    grant: &Grant,
    client: Option<&Path>,
    measure: impl FnOnce(&Session) -> Result<T, Error>,
) -> Result<T, Error> {
    let session = Session::start(
        request.compositor,
        request.compositor_renderer(),
        Outputs::plain(request.outputs),
        None,
        // The program measured chooses its renderer as asked.
        Gpu::Machine,
        |data_home| {
            grant.install(data_home)?;
            if let Some(client) = client
                && request.compositor == Compositor::Kwin
            {
                crate::grant::install_entry(&crate::input::grant(client)?, data_home)?;
            }
            Ok(())
        },
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
fn program_client(session: &Session, request: &Request, cache: Option<&Path>) -> Command {
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

/// With `client` (the input client's executable), runs `measure` with the
/// input `request` gives in `session`: the client's command, and the
/// pointer's steps, each point's output named as the session names it.
fn with_inputs<T>(
    request: &Request,
    session: &Session,
    client: Option<&Path>,
    measure: impl FnOnce(Option<&Inputs<'_>>) -> Result<T, Error>,
) -> Result<T, Error> {
    let Some(executable) = client else {
        return measure(None);
    };
    let pointer = match &request.pointer {
        Some(pointer) => Some(pointer.steps(&session.output_names()?)?),
        None => None,
    };
    let client = || {
        let mut command = session.client(executable.as_os_str());
        command.arg("input");
        command
    };
    measure(Some(&Inputs {
        client: &client,
        pointer: pointer.as_ref(),
    }))
}

/// This binary, which is the input client run as `cargo xtask input`, with
/// links resolved: the executable KWin sees, and grants.
fn input_executable() -> Result<PathBuf, Error> {
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|source| Error::Io {
            what: "find xtask's own executable, the input client".into(),
            source,
        })
}

/// What was measured, where and how.
fn header(request: &Request) -> String {
    let args: Vec<String> = request
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let header = format!(
        "Measured: {} {}\n\
         Compositor: {} (nested, headless, {} output(s); {})\n\
         Renderer: {}; shader cache: {}; {}\n\
         Machine: {}",
        request.program.to_string_lossy(),
        args.join(" "),
        request.compositor.name(),
        request.outputs,
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
    );
    let header = match request.windows {
        Some(n) => {
            format!("{header}\nWindows: {n} expected; start-up ends at the last one's first frame")
        }
        None => header,
    };
    let header = if request.summons.as_ref().is_some_and(|s| s.prepares) {
        format!(
            "{header}\nPrepares (--prepares): the first show waited for the program's first \
             preparation, then a quiet second"
        )
    } else {
        header
    };
    let header = match request.summons.as_ref().and_then(|s| s.typing.as_ref()) {
        Some(typing) => format!(
            "{header}\nTyped after each show: {:?}, a key at a time, each to settled",
            typing.text
        ),
        None => header,
    };
    let header = match &request.pointer {
        Some(pointer) => format!("{header}\nPointer: {}", pointer.describe()),
        None => header,
    };
    match environment_set(request) {
        Some(set) => format!("{header}\nEnvironment set: {set}"),
        None => header,
    }
}
