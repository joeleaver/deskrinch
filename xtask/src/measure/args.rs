//! `cargo xtask measure`'s command line, and the [`Request`] it makes.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use super::{
    Expected, QueryLine, Renderer, Request, ShaderCache, Summons, Typing, parse_env, parse_text,
};
use crate::session::Compositor;

/// `cargo xtask measure`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// The compositor each run's session has.
    #[arg(long, value_enum, default_value = "kwin")]
    compositor: Compositor,
    /// How many runs, each in a fresh session.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=50))]
    runs: u32,
    /// How many outputs each run's session has, 1280x720 each.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
    outputs: u32,
    /// The renderer, through `RINCH_RENDERER` (unset: the caller's
    /// environment decides).
    #[arg(long, value_enum)]
    renderer: Option<Renderer>,
    /// Fail (exit 1), naming what did, unless every run presented with
    /// this renderer, as rinch's own log lines say: a check that a
    /// silent fallback, or a change in rinch's wording, is noticed.
    #[arg(long, value_enum)]
    expect_renderer: Option<Expected>,
    /// The GPU driver's shader cache at each start.
    #[arg(long, value_enum, default_value = "cold")]
    shader_cache: ShaderCache,
    /// Seconds after the first frame before memory is read and the idle
    /// window begins, in both modes: work done after the first frame (an
    /// index built on a thread of its own) is kept out of the window.
    #[arg(long, default_value_t = 5)]
    settle: u64,
    /// Seconds of the idle window. Two minutes catch two ticks of a
    /// clock that shows minutes.
    #[arg(long, default_value_t = 125)]
    idle: u64,
    /// Seconds to wait for the first frame (default 120, or 1200 with
    /// `--instructions`).
    #[arg(long)]
    first_frame_timeout: Option<u64>,
    /// Count instructions under Callgrind instead of measuring CPU time
    /// and memory.
    #[arg(long)]
    instructions: bool,
    /// With `--instructions`: keep each run's Callgrind profile in
    /// `DIR/run-N/`, for `callgrind_annotate`.
    #[arg(long, value_name = "DIR", requires = "instructions")]
    profiles: Option<PathBuf>,
    /// Measure showing instead of starting: the program starts hidden, and
    /// each show is run as the program with these arguments (`show`), from
    /// the command to the first frame after it.
    #[arg(long, value_name = "ARGS", requires = "dismiss")]
    summon: Option<String>,
    /// With `--summon`: the program's arguments that hide it again
    /// (`hide`), after each show.
    #[arg(long, value_name = "ARGS", requires = "summon")]
    dismiss: Option<String>,
    /// With `--summon`: how many shows each run measures; the first is
    /// the cold one.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=20))]
    shows: u32,
    /// With `--summon`: the program prepares its hidden window, so its start
    /// is done only once it has: the first settle waits for its first
    /// prepared frame, and the run fails if none comes within
    /// `--first-frame-timeout`.
    #[arg(long, requires = "summon")]
    prepares: bool,
    /// With `--summon`: type TEXT into the program after each show, a
    /// key at a time, each measured to settled. Needs `--keyboard-line`
    /// and `--query-line`.
    #[arg(
        long = "type",
        value_name = "TEXT",
        requires_all = ["summon", "keyboard_line", "query_line"],
        value_parser = parse_text
    )]
    type_text: Option<String>,
    /// With `--type`: words of the line the program writes once a key
    /// typed reaches where it types; the first key waits for it.
    #[arg(long, value_name = "TEXT", requires = "type_text")]
    keyboard_line: Option<String>,
    /// With `--type`: the line the program writes at the hide, `{}`
    /// where it gives how many characters it was typed; it must give
    /// TEXT's.
    #[arg(
        long,
        value_name = "PATTERN",
        requires = "type_text",
        value_parser = QueryLine::parse
    )]
    query_line: Option<QueryLine>,
    /// Open one window with this app id at the idle window's start, and
    /// keep it open to its end: the idle figures then include what it
    /// costs the program. A run fails if it cannot be opened or is gone
    /// by the end.
    #[arg(long, value_name = "APP_ID", conflicts_with = "summon")]
    window: Option<String>,
    /// The program opens this many windows (one per output, say): read
    /// each from rinch's per-window lines instead of the main window's.
    /// Start-up ends at the last one's first frame; a run fails if the
    /// count differs or a window closes or opens after start-up.
    #[arg(
        long,
        value_name = "N",
        conflicts_with = "summon",
        value_parser = clap::value_parser!(u32).range(1..=16)
    )]
    windows: Option<u32>,
    /// The shell component the program is (its package, such as
    /// deskrinch-panel), when its file name is not the package's: under
    /// KWin it is granted what deskrinch-shell grants that component. A
    /// program named as a component's package is granted without it.
    #[arg(long, value_name = "PACKAGE")]
    component: Option<String>,
    /// Set KEY to VALUE in the measured program's environment, after
    /// what measure sets itself (repeatable). The header and each run's
    /// notes say what was set.
    #[arg(long = "env", value_name = "KEY=VALUE", value_parser = parse_env)]
    env: Vec<(String, String)>,
    /// Measure a program that logged errors, with the lines in each
    /// run's notes, instead of failing the run.
    #[arg(long)]
    allow_errors: bool,
    /// Each run's state directory (`XDG_STATE_HOME`) starts with a copy
    /// of this directory's contents: state the program reads, such as
    /// the record of starts the launcher shows (`deskrinch/starts.toml`).
    #[arg(long, value_name = "DIR")]
    state: Option<PathBuf>,
    /// The program to measure.
    program: OsString,
    /// Its arguments.
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    arguments: Vec<OsString>,
}

impl Args {
    /// What these arguments ask for.
    pub(crate) fn request(self) -> Request {
        let typing = self
            .type_text
            .zip(self.keyboard_line)
            .zip(self.query_line)
            .map(|((text, keyboard_line), query_line)| Typing {
                text,
                keyboard_line,
                query_line,
            });
        let shows = self.shows;
        Request {
            compositor: self.compositor,
            runs: self.runs,
            outputs: self.outputs,
            renderer: self.renderer,
            expect_renderer: self.expect_renderer,
            shader_cache: self.shader_cache,
            settle: Duration::from_secs(self.settle),
            idle: Duration::from_secs(self.idle),
            first_frame_timeout: Duration::from_secs(
                self.first_frame_timeout
                    .unwrap_or(if self.instructions { 1200 } else { 120 }),
            ),
            instructions: self.instructions,
            profiles: self.profiles,
            state: self.state,
            program: self.program,
            args: self.arguments,
            summons: self
                .summon
                .zip(self.dismiss)
                .map(|(summon, dismiss)| Summons {
                    typing,
                    prepares: self.prepares,
                    ..Summons::new(&summon, &dismiss, shows)
                }),
            window: self.window,
            windows: self.windows,
            component: self.component,
            allow_errors: self.allow_errors,
            env: self.env,
        }
    }
}
