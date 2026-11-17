//! Instruction counts, as rinch's performance CI takes them: the program
//! runs under Valgrind's Callgrind, which counts every instruction it
//! executes (`Ir`) on every thread, whatever the machine's load. The count
//! is dumped at the first frame, again after the settle (`--settle`, as in
//! the other mode: work the program does after its first frame, such as
//! building an index on a thread of its own, is kept out of the idle window,
//! and its count is noted), and at the end of the idle window.
//! With `--windows` the first frame is the last of the windows expected
//! ([`super::windows`]): one dump, not one per window. The setup commands,
//! between start-up's end and the settle, have a dump of their own, noted
//! and kept out of every row.
//!
//! Under Callgrind the program runs tens of times slower, so its memory,
//! CPU time and wall time say nothing; this mode reports instructions and
//! frames only. Vulkan drivers run under it (RADV and lavapipe do here),
//! but a driver that needs unknown `ioctl`s may not.

mod dumps;

pub(crate) use dumps::{control, dump};

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::input::{
    CallgrindMeter, InputClient, Inputs, PointerSteps, move_pointer, run_commands, run_setup,
};
use super::output;
use super::probe::{self, Timing};
use super::report::{self, Run, Unit};
use super::scripted::Scripted;
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
    scripted: &Scripted<'_>,
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
        scripted,
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
    /// The setup commands, and the command steps (after the pointer's).
    scripted: &'a Scripted<'a>,
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
    let start = Instant::now();
    loop {
        interrupt::check()?;
        ends.wait(watch, Duration::from_millis(100));
        if ends.ended(watch) {
            break;
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(Error::MeasuredExited {
                program: program.to_owned(),
                status,
                output: watch.last_words(),
            });
        }
        if start.elapsed() >= timing.first_frame {
            return Err(ends.not_ended(program, watch, timing.first_frame));
        }
    }
    let load = probe::load_average();
    let first = dump(control, &pid, out, "first-frame")?;
    let started = match expect {
        Expect::Main => None,
        Expect::Windows(n) => {
            Some(Started::of(watch, n).map_err(|e| windows::error(program, watch, e))?)
        }
    };
    let mut setup = setup_counted(child, watch, profile, program, timing.first_frame)?;
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
    run.notes.append(&mut setup.notes);
    // The steps, after the idle window and its rows; each step zeroed at its
    // start and read from the dump named for it.
    let mut running = Running {
        child,
        watch,
        program,
        timeout: timing.first_frame,
    };
    steps_after_idle(&mut run, &mut running, profile, pointer)?;
    window::end(opened, &mut run)?;
    run.note(format!("load average at {}", ends.end.at()), load);
    probe::end_of_run(&mut run, watch, expect.frames(watch));
    Ok(run)
}

/// Runs the setup commands, if `profile` has any, and counts their
/// instructions in a dump of their own, kept out of every row: what they
/// led to, and that count, as notes.
fn setup_counted(
    child: &mut Child,
    watch: &Watch,
    profile: &Profile<'_>,
    program: &str,
    timeout: Duration,
) -> Result<Run, Error> {
    let mut noted = Run::default();
    let Some(commands) = &profile.scripted.setup else {
        return Ok(noted);
    };
    let pid = child.id().to_string();
    let mut running = Running {
        child,
        watch,
        program,
        timeout,
    };
    run_setup(&mut noted, &mut running, commands)?;
    let setup = dump(profile.control, &pid, profile.out, "setup")?;
    noted.notes.insert(
        0,
        (
            "instructions during the setup commands, kept out of every row".to_owned(),
            format!("{} Ir", report::format(setup, Unit::Count)),
        ),
    );
    Ok(noted)
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
    if let Some(commands) = &profile.scripted.steps {
        let mut meter = CallgrindMeter::new(profile.control, id, profile.out);
        run_commands(run, running, commands, &mut meter)?;
    }
    Ok(())
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
    fn valgrind_writes_the_profile_where_asked() {
        let args = valgrind_args(Path::new("/tmp/x/callgrind.out"));
        assert!(args.contains(&"--callgrind-out-file=/tmp/x/callgrind.out".to_owned()));
        assert!(args.contains(&"--tool=callgrind".to_owned()));
    }
}
