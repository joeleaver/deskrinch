//! Instruction counts, as rinch's performance CI takes them: the program
//! runs under Valgrind's Callgrind, which counts every instruction it
//! executes (`Ir`) on every thread, whatever the machine's load. The count
//! is dumped at the first frame, again after the settle (`--settle`, as in
//! the other mode: work the program does after its first frame, such as
//! building an index on a thread of its own, is kept out of the idle window,
//! and its count is noted), and at the end of the idle window.
//! With `--windows` the first frame is the last of the windows expected
//! ([`super::windows`]): one dump, not one per window.
//!
//! Under Callgrind the program runs tens of times slower, so its memory,
//! CPU time and wall time say nothing; this mode reports instructions and
//! frames only. Vulkan drivers run under it (RADV and lavapipe do here),
//! but a driver that needs unknown `ioctl`s may not.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::input::{
    CallgrindMeter, Commands, InputClient, Inputs, PointerSteps, move_pointer, run_commands,
};
use super::output;
use super::probe::{self, Timing};
use super::report::{self, Run, Unit};
use super::settle::Running;
use super::start_end::{End, Start};
use super::window::{self, Stimulus};
use super::windows::{self, Expect, Started};
use crate::error::Error;
use crate::session::interrupt;
use deskrinch_test_component::Watch;

/// The Valgrind arguments: Callgrind, instruction counts only, one count
/// for all threads, written to `out` (dumps go to `out.1`, `out.2`, ...).
pub(crate) fn valgrind_args(out: &Path) -> Vec<String> {
    vec![
        "--tool=callgrind".to_owned(),
        "--dump-instr=no".to_owned(),
        "--separate-threads=no".to_owned(),
        format!("--callgrind-out-file={}", out.display()),
    ]
}

/// Measures the program `command` runs under Valgrind (a session client
/// whose program is `valgrind` with [`valgrind_args`]), asking for dumps
/// through `control`, a session client for `callgrind_control`, and reading
/// the frames `expect` says; with `inputs`, its pointer's steps after the
/// idle window, each from a dump of its own.
#[expect(
    clippy::too_many_arguments,
    reason = "the session's clients, the program, its profile, and what is asked"
)]
pub(crate) fn measure(
    mut command: Command,
    control: impl Fn() -> Command,
    program: &str,
    out: &Path,
    timing: Timing,
    stimulus: &Stimulus,
    start: Start<'_>,
    inputs: Option<&Inputs<'_>>,
    commands: Option<&Commands<'_>>,
) -> Result<Run, Error> {
    // The client first: its devices are made before the program starts.
    let mut client = inputs.map(Inputs::start).transpose()?;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: "valgrind".to_owned(),
            source,
        })?;
    let watch = Watch::default();
    watch.follow_child(&mut child);
    let profile = Profile {
        control: &control,
        out,
        start,
        commands,
    };
    let pointer = client
        .as_mut()
        .zip(inputs.and_then(|inputs| inputs.pointer));
    let result = run_child(
        &mut child, &watch, &profile, program, timing, stimulus, pointer,
    )
    .and_then(|run| {
        client.take().map(InputClient::finish).transpose()?;
        Ok(run)
    });
    probe::stop(&mut child);
    output::with_output(result, &watch)
}

/// Where the profile is asked for and written, and where start-up ends.
struct Profile<'a> {
    control: &'a dyn Fn() -> Command,
    out: &'a Path,
    start: Start<'a>,
    /// With `--step-command`: the command steps, after the pointer's.
    commands: Option<&'a Commands<'a>>,
}

/// Waits, within `timeout`, for start-up's end as `ends` says; fails if the
/// program exits first or xtask is interrupted.
fn wait_for_start(
    child: &mut Child,
    watch: &Watch,
    ends: &Start<'_>,
    program: &str,
    timeout: Duration,
) -> Result<(), Error> {
    let start = Instant::now();
    loop {
        interrupt::check()?;
        ends.wait(watch, Duration::from_millis(100));
        if ends.ended(watch) {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(Error::MeasuredExited {
                program: program.to_owned(),
                status,
                output: watch.last_words(),
            });
        }
        if start.elapsed() >= timeout {
            return Err(ends.not_ended(program, watch, timeout));
        }
    }
}

fn run_child(
    child: &mut Child,
    watch: &Watch,
    profile: &Profile<'_>,
    program: &str,
    timing: Timing,
    stimulus: &Stimulus,
    pointer: Option<(&mut InputClient, &PointerSteps)>,
) -> Result<Run, Error> {
    let Profile {
        control,
        out,
        start: ends,
        ..
    } = *profile;
    let expect = ends.expect;
    let pid = child.id().to_string();
    wait_for_start(child, watch, &ends, program, timing.first_frame)?;
    let load = probe::load_average();
    let first = dump(control, &pid, out, "first-frame")?;
    let started = match expect {
        Expect::Main => None,
        Expect::Windows(n) => {
            Some(Started::of(watch, n).map_err(|e| windows::error(program, watch, e))?)
        }
    };
    probe::wait(child, watch, program, timing.settle)?;
    let settle = dump(control, &pid, out, "settle")?;
    let frames_before = watch.frames();
    let mark = watch.mark();
    let end = Instant::now() + timing.idle;
    let opened = stimulus.begin()?;
    while Instant::now() < end {
        interrupt::check()?;
        thread::sleep((end - Instant::now()).min(Duration::from_millis(100)));
    }
    let frames_main = watch.frames() - frames_before;
    let since = watch.since(&mark);
    let idle = dump(control, &pid, out, "idle")?;
    let mut run = Run::default();
    if let Some(started) = &started {
        started
            .unchanged(watch)
            .map_err(|e| windows::error(program, watch, e))?;
    }

    push_start(&mut run, ends.end, first, settle);
    run.push("instructions in the idle window (Ir)", Unit::Count, idle);
    let frames = match &started {
        None => {
            run.push("frames in the idle window", Unit::Count, frames_main);
            frames_main
        }
        Some(started) => started.push_idle_frames(&mut run, &since),
    };
    if let Some(per_frame) = idle.checked_div(frames) {
        run.push(
            "instructions per frame in the window (with idle)",
            Unit::Count,
            per_frame,
        );
    }
    run.note(
        "instructions during the settle, kept out of the idle window",
        format!(
            "{} Ir in {} s",
            report::format(settle, Unit::Count),
            timing.settle.as_secs()
        ),
    );
    // The steps, after the idle window and its rows; each step zeroed at its
    // start and read from the dump named for it.
    let mut running = Running {
        child,
        watch,
        program,
        timeout: timing.first_frame,
        neighbours: ends.neighbours,
    };
    steps_after_idle(&mut run, &mut running, profile, pointer)?;
    window::end(opened, &mut run)?;
    run.note(format!("load average at {}", ends.end.at()), load);
    run.note("renderer", watch.renderer());
    run.presented = watch.presented();
    run.drew = expect.frames(watch) > 0 || !watch.windows().is_empty();
    Ok(run)
}

/// The start's rows: the instructions to start-up's `end` (`first`, its
/// dump), and to the end of the settle (`first` and the settle's dump
/// `settle` together). The second is the figure to compare across pins when
/// a start-up thread may finish on either side of the end: its work then
/// moves between the two dumps, and only their sum stays put.
fn push_start(run: &mut Run, end: &End, first: u64, settle: u64) {
    run.push(
        format!("instructions to {} (Ir)", end.to()),
        Unit::Count,
        first,
    );
    run.push(
        "instructions to the end of the settle (Ir)",
        Unit::Count,
        first + settle,
    );
}

/// The pointer's steps, then the command steps, each counted from a dump of
/// its own, after the idle window.
fn steps_after_idle(
    run: &mut Run,
    running: &mut Running<'_>,
    profile: &Profile<'_>,
    pointer: Option<(&mut InputClient, &PointerSteps)>,
) -> Result<(), Error> {
    let id = running.child.id();
    if let Some((client, steps)) = pointer {
        let mut meter = CallgrindMeter::new(profile.control, id, profile.out);
        move_pointer(run, running, client, steps, &mut meter, "", "")?;
    }
    if let Some(commands) = profile.commands {
        let mut meter = CallgrindMeter::new(profile.control, id, profile.out);
        run_commands(run, running, commands, &mut meter)?;
    }
    Ok(())
}

/// Asks Callgrind in process `pid` to write a dump with the hint `label`
/// to the profile `out`, and gives its instruction total ([`totals`]).
pub(crate) fn dump(
    control: &dyn Fn() -> Command,
    pid: &str,
    out: &Path,
    label: &str,
) -> Result<u64, Error> {
    self::control(control, &["--dump", label, pid])?;
    totals(out, label)
}

/// Runs `callgrind_control` (a session client) with `args`: `--dump LABEL
/// PID` writes a dump, `--zero PID` zeroes the counts.
pub(crate) fn control(control: &dyn Fn() -> Command, args: &[&str]) -> Result<(), Error> {
    let status = control()
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| Error::Spawn {
            program: "callgrind_control".to_owned(),
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::CallgrindControl { status })
    }
}

/// The instruction total of the dump of the profile at `out` (Callgrind
/// writes each as `out.N`) whose hint is `label`: the dump that names it in
/// its description (`desc: Trigger: dump LABEL`), whatever its number, so a
/// figure is always read from its own span's dump. Callgrind writes a
/// dump's file when it is asked for it, so the file is waited for.
///
/// A run's hints are unique (`first-frame`, `settle`, `idle`, `show-N`,
/// `hide-N`, `show-N-key-K`, `pointer-K`, `show-N-pointer-K`); two dumps of
/// one profile with the same hint are an error, never a guess.
pub(crate) fn totals(out: &Path, label: &str) -> Result<u64, Error> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let named: Vec<(PathBuf, Option<u64>)> = named_dumps(out)
            .filter(|(_, hint, _)| hint == label)
            .map(|(path, _, total)| (path, total))
            .collect();
        if named.len() > 1 {
            let mut files: Vec<PathBuf> = named.into_iter().map(|(path, _)| path).collect();
            files.sort();
            return Err(Error::CallgrindHintRepeated {
                label: label.to_owned(),
                files,
            });
        }
        if let Some((_, Some(total))) = named.first() {
            return Ok(*total);
        }
        if Instant::now() >= deadline {
            return Err(Error::Read {
                path: out.to_owned(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Callgrind wrote no complete dump with the hint {label:?}"),
                ),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// The dumps of the profile at `out` written so far: each one's file, its
/// hint, and its total once it is complete.
fn named_dumps(out: &Path) -> impl Iterator<Item = (PathBuf, String, Option<u64>)> {
    let prefix = out
        .file_name()
        .map(|name| format!("{}.", name.to_string_lossy()))
        .unwrap_or_default();
    let dir = out.parent().map(Path::to_owned).unwrap_or_default();
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(move |entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|entry| Some((entry.path(), fs::read_to_string(entry.path()).ok()?)))
        .filter_map(|(path, text)| Some((path, hint(&text)?.to_owned(), parse_totals(&text))))
}

/// The hint a dump was asked for with, from its description.
fn hint(profile: &str) -> Option<&str> {
    profile
        .lines()
        .find_map(|line| line.strip_prefix("desc: Trigger: dump "))
        .map(str::trim_end)
}

/// The `totals:` (or, without it, `summary:`) line of a Callgrind profile:
/// its instruction count.
pub(crate) fn parse_totals(profile: &str) -> Option<u64> {
    let find = |key: &str| {
        profile.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
    };
    find("totals:").or_else(|| find("summary:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_the_settle_is_the_start_s_dump_and_the_settle_s_together() {
        let row = |run: &Run, name: &str| {
            run.metrics
                .iter()
                .find(|metric| metric.name == name)
                .map(|metric| metric.value)
        };
        let mut run = Run::default();
        push_start(&mut run, &End::Frame(1), 581_098_231, 9_072_297);
        assert_eq!(
            row(&run, "instructions to first frame (Ir)"),
            Some(581_098_231)
        );
        assert_eq!(
            row(&run, "instructions to the end of the settle (Ir)"),
            Some(590_170_528)
        );
        // Named so for any end.
        let mut run = Run::default();
        push_start(&mut run, &End::Frame(2), 10, 792);
        assert_eq!(row(&run, "instructions to start-up's end (Ir)"), Some(10));
        assert_eq!(
            row(&run, "instructions to the end of the settle (Ir)"),
            Some(802)
        );
    }

    #[test]
    fn reads_the_total_of_a_dump() {
        let profile = "version: 1\ncreator: callgrind-3.26.0\npid: 1309\n\
                       events: Ir\nfn=(1) main\n5 100\nsummary: 2310257585\n\
                       totals: 2310257585\n";
        assert_eq!(parse_totals(profile), Some(2_310_257_585));
        assert_eq!(parse_totals("events: Ir\nsummary: 358\n"), Some(358));
        assert_eq!(parse_totals("events: Ir\n"), None);
    }

    #[test]
    fn valgrind_writes_the_profile_where_asked() {
        let args = valgrind_args(Path::new("/tmp/x/callgrind.out"));
        assert!(args.contains(&"--callgrind-out-file=/tmp/x/callgrind.out".to_owned()));
        assert!(args.contains(&"--tool=callgrind".to_owned()));
    }

    /// A dump with the hint `hint` and the total `total`, as Callgrind
    /// writes one.
    fn dump_text(hint: &str, total: u64) -> String {
        format!(
            "version: 1\ncreator: callgrind-3.26.0\npart: 2\ndesc: I1 cache: \n\
             desc: Trigger: dump {hint}\n\npositions: line\nevents: Ir\nsummary: {total}\n\
             totals: {total}\n"
        )
    }

    #[test]
    fn a_figure_is_read_from_the_dump_that_names_it_whatever_its_number() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("callgrind.out");
        fs::write(dir.path().join("callgrind.out.1"), dump_text("idle", 900)).unwrap();
        fs::write(
            dir.path().join("callgrind.out.3"),
            dump_text("pointer-2", 70),
        )
        .unwrap();
        fs::write(
            dir.path().join("callgrind.out.2"),
            dump_text("pointer-1", 50),
        )
        .unwrap();
        fs::write(dir.path().join("other.out.2"), dump_text("pointer-1", 1)).unwrap();
        assert_eq!(totals(&out, "pointer-1").unwrap(), 50);
        assert_eq!(totals(&out, "pointer-2").unwrap(), 70);
        assert_eq!(totals(&out, "idle").unwrap(), 900);
        assert_eq!(
            hint(&dump_text("show-1-pointer-1", 5)),
            Some("show-1-pointer-1")
        );
        assert_eq!(
            hint("version: 1\ndesc: Trigger: Program termination\n"),
            None
        );
    }

    #[test]
    fn two_dumps_of_one_profile_with_one_hint_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("callgrind.out");
        fs::write(
            dir.path().join("callgrind.out.1"),
            dump_text("pointer-1", 50),
        )
        .unwrap();
        fs::write(
            dir.path().join("callgrind.out.2"),
            dump_text("pointer-1", 70),
        )
        .unwrap();
        let error = totals(&out, "pointer-1").unwrap_err().to_string();
        assert!(error.contains("\"pointer-1\""), "{error}");
        assert!(
            error.contains("callgrind.out.1") && error.contains("callgrind.out.2"),
            "{error}"
        );
    }
}
