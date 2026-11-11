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
//! The first show is the cold one: the first window the process makes.
//! Each later one is a show after a hide, which re-creates the window.
//! The summon command's own cost (a second process) is not counted.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::callgrind;
use super::probe::{self, Timing};
use super::procfs::{self, ThreadSample};
use super::report::{Run, Unit};
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
}

impl Summons {
    /// `summon` and `dismiss` as the program's arguments, split at white
    /// space, and `shows` shows.
    pub(crate) fn new(summon: &str, dismiss: &str, shows: u32) -> Self {
        let words = |text: &str| text.split_whitespace().map(OsString::from).collect();
        Self {
            summon: words(summon),
            dismiss: words(dismiss),
            shows,
        }
    }
}

/// The program run again with `args`: a session client.
pub(crate) type Ask<'a> = &'a dyn Fn(&[OsString]) -> Command;

/// Starts `command` (the program, hidden) and measures each show's CPU
/// time; ends it afterwards.
pub(crate) fn measure(
    command: Command,
    ask: Ask<'_>,
    program: &str,
    timing: Timing,
    summons: &Summons,
) -> Result<Run, Error> {
    let (mut child, watch) = start(command, program)?;
    let result = (|| {
        settle_hidden(&mut child, &watch, program, timing.first_frame)?;
        let mut run = Run::default();
        for show in 1..=summons.shows {
            let (cpu, wall) = one_show(&mut child, &watch, ask, program, timing, summons)?;
            run.push(
                format!("show {show}: CPU from the summon to its first frame"),
                Unit::CpuNs,
                cpu,
            );
            run.push(format!("show {show}: wall time"), Unit::WallNs, wall);
            dismiss(&mut child, &watch, ask, program, timing, summons)?;
        }
        run.note("load average after the shows", probe::load_average());
        Ok(run)
    })();
    probe::stop(&mut child);
    result
}

/// Measures each show's instructions: `command` runs the program under
/// Valgrind ([`callgrind::valgrind_args`] writing to `out`), and `control`
/// is a session client for `callgrind_control`.
pub(crate) fn measure_instructions(
    command: Command,
    control: &dyn Fn() -> Command,
    ask: Ask<'_>,
    program: &str,
    out: &Path,
    timing: Timing,
    summons: &Summons,
) -> Result<Run, Error> {
    let (mut child, watch) = start(command, program)?;
    let result = (|| {
        let pid = child.id().to_string();
        settle_hidden(&mut child, &watch, program, timing.first_frame)?;
        let mut run = Run::default();
        for show in 1..=summons.shows {
            callgrind::control(control, &["--zero", &pid])?;
            let frames = watch.frames();
            summon(ask, &summons.summon)?;
            first_frame_after(&mut child, &watch, program, frames, timing.first_frame)?;
            callgrind::control(control, &["--dump", &format!("show-{show}"), &pid])?;
            run.push(
                format!("show {show}: instructions from the summon to its first frame (Ir)"),
                Unit::Count,
                callgrind::totals(out, show)?,
            );
            dismiss(&mut child, &watch, ask, program, timing, summons)?;
        }
        Ok(run)
    })();
    probe::stop(&mut child);
    result
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

/// The CPU time run between `before` and `after`, a thread that started in
/// between counting from zero.
fn cpu_between(before: &BTreeMap<u32, ThreadSample>, after: &BTreeMap<u32, ThreadSample>) -> u64 {
    after
        .iter()
        .map(|(tid, now)| {
            let then = before.get(tid).map_or(0, |t| t.runtime_ns);
            now.runtime_ns.saturating_sub(then)
        })
        .sum()
}

/// Hides the program again, and waits for it to settle.
fn dismiss(
    child: &mut Child,
    watch: &Watch,
    ask: Ask<'_>,
    program: &str,
    timing: Timing,
    summons: &Summons,
) -> Result<(), Error> {
    summon(ask, &summons.dismiss)?;
    settle(child, watch, program, timing.first_frame)
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
    loop {
        let window_end = Instant::now() + SETTLE_WINDOW;
        while Instant::now() < window_end {
            interrupt::check()?;
            alive(child, watch, program)?;
            std::thread::sleep(Duration::from_millis(100));
        }
        let now = procfs::read_threads(pid);
        if cpu_between(&before, &now) < nanos(SETTLED_CPU) {
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
    }
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

    fn thread(runtime_ns: u64) -> ThreadSample {
        ThreadSample {
            name: "t".into(),
            runtime_ns,
            voluntary_switches: 0,
            involuntary_switches: 0,
        }
    }

    #[test]
    fn cpu_between_counts_new_threads_from_zero() {
        let before = BTreeMap::from([(1, thread(100)), (2, thread(50))]);
        let after = BTreeMap::from([(1, thread(160)), (2, thread(50)), (3, thread(30))]);
        assert_eq!(cpu_between(&before, &after), 60 + 30);
    }

    #[test]
    fn a_command_is_written_as_its_words() {
        assert_eq!(words(&["show".into(), "--now".into()]), "show --now");
    }
}
