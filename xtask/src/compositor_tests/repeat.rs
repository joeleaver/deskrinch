//! `--repeat N --into DIR`: the same run, N times, each in fresh sessions.
//!
//! Each run is `cargo xtask compositor-tests` again, as asked but without
//! the repeat ([`Args::once`]): its own process, building (Cargo finds
//! nothing to do after the first) and starting its sessions afresh, its
//! standard output and error together in `DIR/run-NN.log`. After each, the
//! run says how it went; at the end, how many passed and failed, and the
//! files of those that failed.
//!
//! A run that overruns its limit (`--run-timeout`) is ended
//! ([`bounded`]), counted as failed, and noted at the end of its file; the
//! repeat goes on. A run that was interrupted from outside (it exits 130,
//! as xtask does on `SIGINT`) ends the repeat.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use super::args::Args;
use super::bounded::{self, GRACE, Stopped, Waited};
use crate::Outcome;
use crate::error::Error;
use crate::run_in_session::INTERRUPTED;

/// Runs `args`' run `times` times, each within `limit`, each run's output in
/// a file under `into`.
pub(super) fn run(args: &Args, times: u32, into: &Path, limit: Duration) -> Result<Outcome, Error> {
    prepare(into)?;
    let xtask = std::env::current_exe().map_err(|source| Error::Io {
        what: "find xtask's own executable".into(),
        source,
    })?;
    let once = args.once();
    let mut tally = Tally::default();
    for run in 1..=times {
        let file = into.join(log_name(run, times));
        let waited = run_once(&xtask, &once, &file, limit)?;
        let ended = Ended::of(waited, limit);
        if let Ended::TimedOut { .. } = ended {
            note(&file, &ended)?;
        }
        println!(
            "compositor-tests: run {run} of {times}: {}; {}",
            ended.said(),
            file.display()
        );
        if ended == Ended::Interrupted {
            return Err(Error::Interrupted);
        }
        tally.record(&ended, file);
    }
    println!("compositor-tests: {}", tally.said());
    Ok(tally.outcome())
}

/// Makes `into` if it is missing; fails unless it is empty, so that runs of
/// two repeats are never mixed.
fn prepare(into: &Path) -> Result<(), Error> {
    fs::create_dir_all(into).map_err(|source| Error::Io {
        what: format!("make {}", into.display()),
        source,
    })?;
    let mut entries = fs::read_dir(into).map_err(|source| Error::Io {
        what: format!("read {}", into.display()),
        source,
    })?;
    match entries.next() {
        None => Ok(()),
        Some(_) => Err(Error::RepeatIntoNotEmpty(into.to_owned())),
    }
}

/// Runs `xtask` with `once` within `limit`, its output to `file`; how it
/// ended.
fn run_once(
    xtask: &Path,
    once: &[std::ffi::OsString],
    file: &Path,
    limit: Duration,
) -> Result<Waited, Error> {
    let out = File::create(file).map_err(io("create", file))?;
    let err = out.try_clone().map_err(io("share", file))?;
    let mut child = Command::new(xtask)
        .args(once)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .map_err(|source| Error::Spawn {
            program: xtask.display().to_string(),
            source,
        })?;
    bounded::wait(&mut child, limit, GRACE)
}

/// Adds what the repeat did to an overrunning run at the end of its `file`,
/// once the run has ended.
fn note(file: &Path, ended: &Ended) -> Result<(), Error> {
    let mut log = OpenOptions::new()
        .append(true)
        .open(file)
        .map_err(io("open", file))?;
    writeln!(log, "compositor-tests: repeat: this run {}", ended.said()).map_err(io("write", file))
}

/// An [`Error::Io`] about `file`, for `what` was being done to it.
fn io(what: &str, file: &Path) -> impl FnOnce(std::io::Error) -> Error {
    let what = format!("{what} {}", file.display());
    move |source| Error::Io { what, source }
}

/// The file of run `run` of `times`, numbered to sort in order.
fn log_name(run: u32, times: u32) -> String {
    let width = times.to_string().len();
    format!("run-{run:0width$}.log")
}

/// How one run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ended {
    Passed,
    /// Failed, as its exit status says.
    Failed(String),
    /// Overran its limit, and was ended as `stopped` says.
    TimedOut {
        limit: Duration,
        stopped: Stopped,
    },
    /// Interrupted from outside: the repeat ends.
    Interrupted,
}

impl Ended {
    /// How a run waited for within `limit` ended. A run the repeat stopped
    /// timed out, whatever its status (130 when the interrupt ended it).
    fn of(waited: Waited, limit: Duration) -> Self {
        if let Some(stopped) = waited.stopped {
            return Self::TimedOut { limit, stopped };
        }
        Self::of_status(waited.status)
    }

    fn of_status(status: ExitStatus) -> Self {
        match status.code() {
            Some(0) => Self::Passed,
            Some(code) if code == i32::from(INTERRUPTED) => Self::Interrupted,
            _ => Self::Failed(status.to_string()),
        }
    }

    fn said(&self) -> String {
        match self {
            Self::Passed => "passed".into(),
            Self::Failed(status) => format!("failed ({status})"),
            Self::TimedOut { limit, stopped } => {
                let how = match stopped {
                    Stopped::Interrupted => {
                        "it was interrupted (SIGINT) and ended its sessions".to_owned()
                    }
                    Stopped::Killed => format!(
                        "it was interrupted (SIGINT), had not ended {} s later, and was killed \
                         (SIGKILL); its sessions' supervisors end what it left",
                        GRACE.as_secs()
                    ),
                };
                format!("failed: timed out after {} s; {how}", limit.as_secs())
            }
            Self::Interrupted => "interrupted".into(),
        }
    }
}

/// The runs' count: how many passed, and the files of those that failed.
#[derive(Debug, Default)]
struct Tally {
    passed: u32,
    failed: Vec<PathBuf>,
}

impl Tally {
    fn record(&mut self, ended: &Ended, file: PathBuf) {
        match ended {
            Ended::Passed => self.passed += 1,
            Ended::Failed(_) | Ended::TimedOut { .. } | Ended::Interrupted => {
                self.failed.push(file);
            }
        }
    }

    fn said(&self) -> String {
        let count = format!("{} passed, {} failed", self.passed, self.failed.len());
        if self.failed.is_empty() {
            return count;
        }
        let files: Vec<_> = self
            .failed
            .iter()
            .map(|file| file.display().to_string())
            .collect();
        format!("{count}: {}", files.join(", "))
    }

    fn outcome(&self) -> Outcome {
        if self.failed.is_empty() {
            Outcome::Passed
        } else {
            Outcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    const LIMIT: Duration = Duration::from_mins(15);

    fn exited(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn by_itself(status: ExitStatus) -> Ended {
        Ended::of(
            Waited {
                status,
                stopped: None,
            },
            LIMIT,
        )
    }

    #[test]
    fn runs_are_counted_and_any_failure_fails() {
        let mut tally = Tally::default();
        tally.record(&by_itself(exited(0)), "/r/run-1.log".into());
        assert_eq!(tally.said(), "1 passed, 0 failed");
        assert_eq!(tally.outcome(), Outcome::Passed);
        tally.record(&by_itself(exited(1)), "/r/run-2.log".into());
        tally.record(&by_itself(exited(0)), "/r/run-3.log".into());
        tally.record(&by_itself(exited(70)), "/r/run-4.log".into());
        assert_eq!(
            tally.said(),
            "2 passed, 2 failed: /r/run-2.log, /r/run-4.log"
        );
        assert_eq!(tally.outcome(), Outcome::Failed);
    }

    #[test]
    fn how_a_run_ended_is_its_status() {
        assert_eq!(by_itself(exited(0)), Ended::Passed);
        assert_eq!(by_itself(exited(130)), Ended::Interrupted);
        assert_eq!(by_itself(exited(1)).said(), "failed (exit status: 1)");
        // Killed by a signal: a failure, not an interrupt of the repeat.
        assert!(matches!(
            by_itself(ExitStatus::from_raw(9)),
            Ended::Failed(_)
        ));
    }

    #[test]
    fn a_run_that_timed_out_failed_and_the_repeat_goes_on() {
        // Ended by the repeat's interrupt, it exits 130 as an interrupted
        // xtask does: still a timeout, not an interrupt of the repeat.
        let interrupted = Ended::of(
            Waited {
                status: exited(130),
                stopped: Some(Stopped::Interrupted),
            },
            LIMIT,
        );
        assert_eq!(
            interrupted,
            Ended::TimedOut {
                limit: LIMIT,
                stopped: Stopped::Interrupted
            }
        );
        assert_ne!(interrupted, Ended::Interrupted, "the repeat goes on");
        assert_eq!(
            interrupted.said(),
            "failed: timed out after 900 s; it was interrupted (SIGINT) and ended its sessions"
        );
        let killed = Ended::of(
            Waited {
                status: ExitStatus::from_raw(9),
                stopped: Some(Stopped::Killed),
            },
            LIMIT,
        );
        assert!(killed.said().starts_with("failed: timed out after 900 s;"));
        assert!(killed.said().contains("was killed (SIGKILL)"));
        let mut tally = Tally::default();
        tally.record(&interrupted, "/r/run-1.log".into());
        tally.record(&by_itself(exited(0)), "/r/run-2.log".into());
        tally.record(&killed, "/r/run-3.log".into());
        assert_eq!(
            tally.said(),
            "1 passed, 2 failed: /r/run-1.log, /r/run-3.log"
        );
        assert_eq!(tally.outcome(), Outcome::Failed);
    }

    #[test]
    fn a_timeout_is_noted_at_the_end_of_the_runs_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run-1.log");
        fs::write(&file, "running 7 tests\n").unwrap();
        let ended = Ended::TimedOut {
            limit: LIMIT,
            stopped: Stopped::Interrupted,
        };
        note(&file, &ended).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!(
                "running 7 tests\ncompositor-tests: repeat: this run {}\n",
                ended.said()
            )
        );
    }

    #[test]
    fn the_files_sort_in_order() {
        assert_eq!(log_name(3, 9), "run-3.log");
        assert_eq!(log_name(3, 20), "run-03.log");
        assert_eq!(log_name(100, 100), "run-100.log");
    }

    #[test]
    fn the_directory_is_made_and_must_be_empty() {
        let dir = tempfile::tempdir().unwrap();
        let into = dir.path().join("runs");
        prepare(&into).unwrap();
        prepare(&into).unwrap();
        fs::write(into.join("run-1.log"), "").unwrap();
        assert!(matches!(
            prepare(&into),
            Err(Error::RepeatIntoNotEmpty(at)) if at == into
        ));
    }
}
