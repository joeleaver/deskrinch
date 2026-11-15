//! `--summon`: what it costs a program that starts hidden to show itself.
//!
//! The program is started hidden and left to settle. Then, for each show,
//! the summon command (the program itself, run again with the summon
//! arguments, as a key binding would run it) asks it to show, and the cost
//! is taken from just before that command starts to the first frame the
//! program presents after it: CPU time of all its threads, threads that
//! ended in between included, or with `--instructions` the instructions
//! Callgrind counts in that span. The dismiss command hides it again, and
//! it is left to settle before the next show.
//!
//! Settled means a window in which its threads ran almost nothing and rinch
//! prepared no frame for its hidden window (`[PERF] prepared N:`), so a show
//! is never summoned while a preparation goes on; the report gives the
//! prepared frames before each show. With `--instructions` each hide is
//! counted too, from the dismiss command to settled, with the prepared
//! frames in that span: what a preparation after a hide costs.
//!
//! Before the first show, the program's memory is read once, hidden and
//! settled (CPU time only; under Callgrind memory says nothing).
//!
//! With `--type`, a query is typed after each show's first frame, a key at
//! a time, and each key measured ([`typing`]), before the dismiss.
//!
//! The first show is the cold one: the first window the process makes.
//! Each later one is a show after a hide, which re-creates the window.
//! The summon command's own cost (a second process) is not counted.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::callgrind;
use super::output;
use super::probe::{self, Timing};
use super::procfs::{self, cpu_between};
use super::report::{Run, Unit};
use super::typing::{self, CallgrindMeter, CpuMeter, Typing, Typist};
use crate::error::Error;
use crate::session::interrupt;
use deskrinch_test_component::Watch;

/// How often threads are sampled during a show.
const SAMPLE: Duration = Duration::from_millis(2);
/// A program counts as settled when its threads ran less than this much
/// CPU time over [`SETTLE_WINDOW`].
const SETTLED_CPU: Duration = Duration::from_millis(5);
/// The window [`SETTLED_CPU`] is measured over.
const SETTLE_WINDOW: Duration = Duration::from_secs(1);

/// What `--summon` asks for.
#[derive(Debug, Clone)]
pub(crate) struct Summons {
    /// The program's arguments that show it (`show`, `toggle`).
    pub(crate) summon: Vec<OsString>,
    /// Its arguments that hide it again (`hide`).
    pub(crate) dismiss: Vec<OsString>,
    /// How many shows each run measures.
    pub(crate) shows: u32,
    /// With `--type`: what is typed after each show ([`typing`]).
    pub(crate) typing: Option<Typing>,
}

impl Summons {
    /// `summon` and `dismiss` as the program's arguments, split at white
    /// space, `shows` shows, and what is typed after each, if anything.
    pub(crate) fn new(summon: &str, dismiss: &str, shows: u32, typing: Option<Typing>) -> Self {
        let words = |text: &str| text.split_whitespace().map(OsString::from).collect();
        Self {
            summon: words(summon),
            dismiss: words(dismiss),
            shows,
            typing,
        }
    }
}

/// The program run again with `args`: a session client.
pub(crate) type Ask<'a> = &'a dyn Fn(&[OsString]) -> Command;

/// The measured program while it runs: what waiting on it needs.
pub(crate) struct Running<'a> {
    pub(crate) child: &'a mut Child,
    pub(crate) watch: &'a Watch,
    pub(crate) program: &'a str,
    /// How long whatever it is waited for may take.
    pub(crate) timeout: Duration,
}

impl Running<'_> {
    /// Waits for it to settle.
    pub(crate) fn settle(&mut self) -> Result<(), Error> {
        settle(self.child, self.watch, self.program, self.timeout)
    }

    /// Fails if it has exited.
    pub(crate) fn alive(&mut self) -> Result<(), Error> {
        alive(self.child, self.watch, self.program)
    }
}

/// Starts the typist if `summons` types: before the program can show,
/// since a headless wlroots seat gives a window that maps while it has no
/// keyboard none of the keyboard.
fn start_typist(summons: &Summons, typist: &dyn Fn() -> Command) -> Result<Option<Typist>, Error> {
    summons
        .typing
        .as_ref()
        .map(|_| Typist::start(typist()))
        .transpose()
}

/// Starts `command` (the program, hidden) and measures each show's CPU
/// time, and with `--type` each key's (`typist` making the typist's
/// command); ends it afterwards.
pub(crate) fn measure(
    command: Command,
    ask: Ask<'_>,
    typist: &dyn Fn() -> Command,
    program: &str,
    timing: Timing,
    summons: &Summons,
) -> Result<Run, Error> {
    let (mut child, watch) = start(command, program)?;
    let result = (|| {
        let mut typist = start_typist(summons, typist)?;
        settle_hidden(&mut child, &watch, program, timing.first_frame)?;
        let mut run = Run::default();
        // What it holds hidden, settled, before the first show: with a
        // window it prepared, what preparing costs.
        probe::memory(&mut run, child.id(), program)?;
        let pid = child.id();
        for show in 1..=summons.shows {
            push_prepared(&mut run, show, &watch);
            let mark = watch.lines().len();
            let (cpu, wall) = one_show(&mut child, &watch, ask, program, timing, summons)?;
            run.push(
                format!("show {show}: CPU from the summon to its first frame"),
                Unit::CpuNs,
                cpu,
            );
            run.push(format!("show {show}: wall time"), Unit::WallNs, wall);
            let mut running = Running {
                child: &mut child,
                watch: &watch,
                program,
                timeout: timing.first_frame,
            };
            if let (Some(typing), Some(typist)) = (&summons.typing, typist.as_mut()) {
                let mut meter = CpuMeter::new(pid);
                typing::after_show(
                    &mut run,
                    &mut running,
                    typist,
                    typing,
                    &mut meter,
                    mark,
                    show,
                )?;
            }
            dismiss(&mut running, ask, summons)?;
            if let Some(typing) = &summons.typing {
                typing::check_query_line(&mut running, typing, mark, show)?;
            }
        }
        typist.map(Typist::finish).transpose()?;
        run.note("load average after the shows", probe::load_average());
        run.presented = watch.presented();
        Ok(run)
    })();
    probe::stop(&mut child);
    output::with_output(result, &watch)
}

/// Measures each show's instructions: `command` runs the program under
/// Valgrind ([`callgrind::valgrind_args`] writing to `out`), and `control`
/// is a session client for `callgrind_control`.
#[expect(
    clippy::too_many_arguments,
    reason = "the session's three clients, the program, its profile, and what is asked"
)]
pub(crate) fn measure_instructions(
    command: Command,
    control: &dyn Fn() -> Command,
    ask: Ask<'_>,
    typist: &dyn Fn() -> Command,
    program: &str,
    out: &Path,
    timing: Timing,
    summons: &Summons,
) -> Result<Run, Error> {
    let (mut child, watch) = start(command, program)?;
    let result = (|| {
        let mut typist = start_typist(summons, typist)?;
        let id = child.id();
        let pid = id.to_string();
        settle_hidden(&mut child, &watch, program, timing.first_frame)?;
        let mut run = Run::default();
        // Callgrind numbers its dumps in the order they are asked for: a
        // show's, each key's, then its hide's.
        let mut dumps = 0;
        for show in 1..=summons.shows {
            push_prepared(&mut run, show, &watch);
            let mark = watch.lines().len();
            callgrind::control(control, &["--zero", &pid])?;
            let frames = watch.frames();
            summon(ask, &summons.summon)?;
            first_frame_after(&mut child, &watch, program, frames, timing.first_frame)?;
            callgrind::control(control, &["--dump", &format!("show-{show}"), &pid])?;
            dumps += 1;
            run.push(
                format!("show {show}: instructions from the summon to its first frame (Ir)"),
                Unit::Count,
                callgrind::totals(out, dumps)?,
            );
            let mut running = Running {
                child: &mut child,
                watch: &watch,
                program,
                timeout: timing.first_frame,
            };
            if let (Some(typing), Some(typist)) = (&summons.typing, typist.as_mut()) {
                let mut meter = CallgrindMeter::new(control, id, out, &mut dumps);
                typing::after_show(
                    &mut run,
                    &mut running,
                    typist,
                    typing,
                    &mut meter,
                    mark,
                    show,
                )?;
            }
            // The hide, to settled: what the program does once dismissed,
            // a preparation of its hidden window included.
            callgrind::control(control, &["--zero", &pid])?;
            let prepared = watch.prepared();
            dismiss(&mut running, ask, summons)?;
            callgrind::control(control, &["--dump", &format!("hide-{show}"), &pid])?;
            dumps += 1;
            run.push(
                format!("hide {show}: instructions from the dismiss to settled (Ir)"),
                Unit::Count,
                callgrind::totals(out, dumps)?,
            );
            run.push(
                format!("hide {show}: prepared frames from the dismiss to settled"),
                Unit::Count,
                watch.prepared() - prepared,
            );
            if let Some(typing) = &summons.typing {
                typing::check_query_line(&mut running, typing, mark, show)?;
            }
        }
        typist.map(Typist::finish).transpose()?;
        run.presented = watch.presented();
        Ok(run)
    })();
    probe::stop(&mut child);
    output::with_output(result, &watch)
}

/// Records the main window's prepared frames so far, before show `show`.
fn push_prepared(run: &mut Run, show: u32, watch: &Watch) {
    run.push(
        format!("show {show}: prepared frames before it, in all"),
        Unit::Count,
        watch.prepared(),
    );
}

fn start(mut command: Command, program: &str) -> Result<(Child, Watch), Error> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: program.to_owned(),
            source,
        })?;
    let watch = Watch::default();
    watch.follow_child(&mut child);
    Ok((child, watch))
}

/// One show: the summon, and the CPU time and wall time to the first frame
/// after it.
fn one_show(
    child: &mut Child,
    watch: &Watch,
    ask: Ask<'_>,
    program: &str,
    timing: Timing,
    summons: &Summons,
) -> Result<(u64, u64), Error> {
    let pid = child.id();
    let before = procfs::read_threads(pid);
    let frames = watch.frames();
    let start = Instant::now();
    let mut seen = before.clone();
    let mut asked = ask(&summons.summon)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: program.to_owned(),
            source,
        })?;
    let mut summoned = false;
    while !watch.wait_frame_after(frames, SAMPLE) {
        interrupt::check()?;
        seen.extend(procfs::read_threads(pid));
        if !summoned && let Ok(Some(status)) = asked.try_wait() {
            summoned = true;
            if !status.success() {
                return Err(summon_failed(&summons.summon, status));
            }
        }
        alive(child, watch, program)?;
        if start.elapsed() >= timing.first_frame {
            return Err(Error::NoFirstFrame {
                program: program.to_owned(),
                waited: timing.first_frame,
                output: watch.tail(),
            });
        }
    }
    seen.extend(procfs::read_threads(pid));
    let wall = nanos(start.elapsed());
    if !summoned {
        let status = asked.wait().map_err(|source| Error::Spawn {
            program: program.to_owned(),
            source,
        })?;
        if !status.success() {
            return Err(summon_failed(&summons.summon, status));
        }
    }
    Ok((cpu_between(&before, &seen), wall))
}

/// Hides the program again, and waits for it to settle.
fn dismiss(running: &mut Running<'_>, ask: Ask<'_>, summons: &Summons) -> Result<(), Error> {
    summon(ask, &summons.dismiss)?;
    running.settle()
}

/// Runs the program with `args` and waits for it.
fn summon(ask: Ask<'_>, args: &[OsString]) -> Result<(), Error> {
    let status = ask(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| Error::Spawn {
            program: words(args),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(summon_failed(args, status))
    }
}

/// Waits for a frame after the first `frames`.
fn first_frame_after(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    frames: u64,
    timeout: Duration,
) -> Result<(), Error> {
    let start = Instant::now();
    while !watch.wait_frame_after(frames, Duration::from_millis(100)) {
        interrupt::check()?;
        alive(child, watch, program)?;
        if start.elapsed() >= timeout {
            return Err(Error::NoFirstFrame {
                program: program.to_owned(),
                waited: timeout,
                output: watch.tail(),
            });
        }
    }
    Ok(())
}

/// Waits for the program, just started, to settle, hidden: a frame before
/// the first summon means it did not start hidden.
fn settle_hidden(
    child: &mut Child,
    watch: &Watch,
    program: &str,
    timeout: Duration,
) -> Result<(), Error> {
    settle(child, watch, program, timeout)?;
    if watch.frames() > 0 {
        return Err(Error::ShownBeforeSummon {
            program: program.to_owned(),
            output: watch.tail(),
        });
    }
    Ok(())
}

/// Waits until the program's threads run less than [`SETTLED_CPU`] over
/// [`SETTLE_WINDOW`].
fn settle(child: &mut Child, watch: &Watch, program: &str, timeout: Duration) -> Result<(), Error> {
    let pid = child.id();
    let start = Instant::now();
    let mut before = procfs::read_threads(pid);
    let mut prepared = watch.prepared();
    loop {
        let window_end = Instant::now() + SETTLE_WINDOW;
        while Instant::now() < window_end {
            interrupt::check()?;
            alive(child, watch, program)?;
            std::thread::sleep(Duration::from_millis(100));
        }
        let now = procfs::read_threads(pid);
        let prepared_now = watch.prepared();
        if settled(cpu_between(&before, &now), prepared, prepared_now) {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            return Err(Error::NeverSettled {
                program: program.to_owned(),
                waited: timeout,
                output: watch.tail(),
            });
        }
        before = now;
        prepared = prepared_now;
    }
}

/// Whether a settle window was quiet: its threads ran less than
/// [`SETTLED_CPU`] (`cpu_ns`), and no prepared frame came in it (the main
/// window's prepared frames went from `prepared_before` to
/// `prepared_after`). A preparation queued but not yet run shows in
/// neither until it runs; one that ran shows in the second however little
/// CPU it took, so a show is never summoned while a preparation goes on.
fn settled(cpu_ns: u64, prepared_before: u64, prepared_after: u64) -> bool {
    cpu_ns < nanos(SETTLED_CPU) && prepared_after == prepared_before
}

/// Fails if the program has exited.
fn alive(child: &mut Child, watch: &Watch, program: &str) -> Result<(), Error> {
    match child.try_wait() {
        Ok(Some(status)) => Err(Error::MeasuredExited {
            program: program.to_owned(),
            status,
            output: watch.last_words(),
        }),
        _ => Ok(()),
    }
}

fn summon_failed(args: &[OsString], status: std::process::ExitStatus) -> Error {
    Error::SummonFailed {
        command: words(args),
        status,
    }
}

fn words(args: &[OsString]) -> String {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settled_wants_quiet_threads_and_no_prepared_frame() {
        let quiet = nanos(SETTLED_CPU) - 1;
        assert!(settled(quiet, 2, 2));
        assert!(!settled(quiet, 2, 3), "a preparation ran in the window");
        assert!(!settled(nanos(SETTLED_CPU), 2, 2), "the threads ran");
    }

    #[test]
    fn a_command_is_written_as_its_words() {
        assert_eq!(words(&["show".into(), "--now".into()]), "show --now");
    }
}
