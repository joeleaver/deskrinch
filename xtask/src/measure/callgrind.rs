//! Instruction counts, as rinch's performance CI takes them: the program
//! runs under Valgrind's Callgrind, which counts every instruction it
//! executes (`Ir`) on every thread, whatever the machine's load. The count
//! is dumped at the first frame and again at the end of an idle window.
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

use super::probe::{self, Timing};
use super::report::{Run, Unit};
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
/// through `control`, a session client for `callgrind_control`.
pub(crate) fn measure(
    mut command: Command,
    control: impl Fn() -> Command,
    program: &str,
    out: &Path,
    timing: Timing,
) -> Result<Run, Error> {
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
    let result = run_child(&mut child, &watch, &control, program, out, timing);
    probe::stop(&mut child);
    result
}

fn run_child(
    child: &mut Child,
    watch: &Watch,
    control: &dyn Fn() -> Command,
    program: &str,
    out: &Path,
    timing: Timing,
) -> Result<Run, Error> {
    let pid = child.id().to_string();
    let start = Instant::now();
    loop {
        interrupt::check()?;
        if watch.wait_first_frame(Duration::from_millis(100)).is_some() {
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
            return Err(Error::NoFirstFrame {
                program: program.to_owned(),
                waited: timing.first_frame,
                output: watch.tail(),
            });
        }
    }
    let load = probe::load_average();
    dump(control, &pid, "first-frame")?;
    let frames_before = watch.frames();
    let end = Instant::now() + timing.idle;
    while Instant::now() < end {
        interrupt::check()?;
        thread::sleep((end - Instant::now()).min(Duration::from_millis(100)));
    }
    let frames = watch.frames() - frames_before;
    dump(control, &pid, "idle")?;

    let first = totals(out, 1)?;
    let idle = totals(out, 2)?;
    let mut run = Run::default();
    run.push("instructions to first frame (Ir)", Unit::Count, first);
    run.push("instructions in the idle window (Ir)", Unit::Count, idle);
    run.push("frames in the idle window", Unit::Count, frames);
    if let Some(per_frame) = idle.checked_div(frames) {
        run.push(
            "instructions per frame in the window (with idle)",
            Unit::Count,
            per_frame,
        );
    }
    run.note("load average at the first frame", load);
    run.note("renderer", watch.renderer());
    run.presented = watch.presented();
    Ok(run)
}

/// Asks Callgrind in process `pid` to write a dump named `label`.
fn dump(control: &dyn Fn() -> Command, pid: &str, label: &str) -> Result<(), Error> {
    self::control(control, &["--dump", label, pid])
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

/// The instruction total of dump `n` of the profile at `out`. Callgrind
/// writes a dump's file when it is asked for it, so the file is waited for.
pub(crate) fn totals(out: &Path, n: u32) -> Result<u64, Error> {
    let path = dump_path(out, n);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(text) = fs::read_to_string(&path)
            && let Some(total) = parse_totals(&text)
        {
            return Ok(total);
        }
        if Instant::now() >= deadline {
            return Err(Error::Read {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Callgrind wrote no complete dump",
                ),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Where Callgrind writes dump `n` of the profile `out`: `out.n`.
pub(crate) fn dump_path(out: &Path, n: u32) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(format!(".{n}"));
    PathBuf::from(name)
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
    fn reads_the_total_of_a_dump() {
        let profile = "version: 1\ncreator: callgrind-3.26.0\npid: 1309\n\
                       events: Ir\nfn=(1) main\n5 100\nsummary: 2310257585\n\
                       totals: 2310257585\n";
        assert_eq!(parse_totals(profile), Some(2_310_257_585));
        assert_eq!(parse_totals("events: Ir\nsummary: 358\n"), Some(358));
        assert_eq!(parse_totals("events: Ir\n"), None);
    }

    #[test]
    fn dumps_are_numbered_after_the_output_file() {
        let args = valgrind_args(Path::new("/tmp/x/callgrind.out"));
        assert!(args.contains(&"--callgrind-out-file=/tmp/x/callgrind.out".to_owned()));
        assert!(args.contains(&"--tool=callgrind".to_owned()));
        assert_eq!(
            dump_path(Path::new("/tmp/x/callgrind.out"), 2),
            Path::new("/tmp/x/callgrind.out.2")
        );
    }
}
