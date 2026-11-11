//! A command run as root, for at most a given time.
//!
//! The limit is enforced inside the privilege boundary: `sudo timeout …
//! <command>`. xtask could not stop the command itself: it may not signal a
//! root process, and `sudo` does not pass on a signal sent from its own
//! process group, which xtask is in. `timeout`, running as root, can.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::classify::Ended;
use crate::error::Error;

/// How long `timeout` waits after `SIGTERM` before it sends `SIGKILL`.
const KILL_AFTER: Duration = Duration::from_secs(10);

/// `timeout`'s status when the command outlived the limit and ended on
/// `SIGTERM`, and when it needed `SIGKILL` (128 + 9).
const TIMED_OUT: [i32; 2] = [124, 137];

/// The command line `sudo` runs: `program` with `args`, under `timeout`.
fn command_line(program: &str, args: &[String], limit: Duration) -> Vec<String> {
    let mut line = vec![
        "timeout".to_owned(),
        "--signal=TERM".to_owned(),
        format!("--kill-after={}s", KILL_AFTER.as_secs()),
        format!("{}s", limit.as_secs()),
        program.to_owned(),
    ];
    line.extend(args.iter().cloned());
    line
}

/// Runs `program` with `args` as root, printing its output as it comes;
/// returns how it ended and what it printed. It ends within `limit` (plus
/// [`KILL_AFTER`] for a command that ignores `SIGTERM`).
pub(super) fn run_as_root(
    program: &str,
    args: &[String],
    limit: Duration,
) -> Result<(Ended, String), Error> {
    let mut child = Command::new("sudo")
        .args(command_line(program, args, limit))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: format!("sudo {program}"),
            source,
        })?;
    let output = Arc::new(Mutex::new(String::new()));
    let readers = [
        child.stdout.take().map(|out| echo(out, &output)),
        child.stderr.take().map(|err| echo(err, &output)),
    ];
    let status = child.wait().map_err(|source| Error::Io {
        what: format!("wait for {program}"),
        source,
    })?;
    for reader in readers.into_iter().flatten() {
        // A reader only copies lines; a panic in it already reported itself.
        let _joined = reader.join();
    }
    let output = output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    Ok((ended(status, limit), output))
}

/// A thread that prints `stream`'s lines and keeps them in `output`.
fn echo(stream: impl Read + Send + 'static, output: &Arc<Mutex<String>>) -> JoinHandle<()> {
    let output = Arc::clone(output);
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            println!("{line}");
            let mut output = output.lock().unwrap_or_else(PoisonError::into_inner);
            output.push_str(&line);
            output.push('\n');
        }
    })
}

/// How a run under `timeout` with `limit` that ended with `status` ended.
fn ended(status: ExitStatus, limit: Duration) -> Ended {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _) if TIMED_OUT.contains(&code) => Ended::CutOff(limit),
        (Some(code), _) => Ended::Exited(code),
        (None, Some(signal)) => Ended::Signalled(signal),
        (None, None) => Ended::Exited(-1),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::process::ExitStatusExt as _;
    use std::time::Instant;

    use super::*;

    #[test]
    fn the_limit_is_timeouts() {
        let line = command_line("apt-get", &["update".to_owned()], Duration::from_mins(5));
        assert_eq!(
            line.join(" "),
            "timeout --signal=TERM --kill-after=10s 300s apt-get update"
        );
    }

    #[test]
    fn timeouts_statuses_are_a_cut_off() {
        let limit = Duration::from_secs(1);
        let exited = |code: i32| ExitStatus::from_raw(code << 8);
        assert_eq!(ended(exited(124), limit), Ended::CutOff(limit));
        assert_eq!(ended(exited(137), limit), Ended::CutOff(limit));
        assert_eq!(ended(exited(100), limit), Ended::Exited(100));
        assert_eq!(ended(exited(0), limit), Ended::Exited(0));
        assert_eq!(ended(ExitStatus::from_raw(9), limit), Ended::Signalled(9));
    }

    /// Whether a process runs whose command line is `words`.
    fn running(words: &[&str]) -> bool {
        let wanted = words.join("\0") + "\0";
        fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| fs::read(entry.path().join("cmdline")).ok())
            .any(|cmdline| cmdline == wanted.as_bytes())
    }

    /// `sudo` for real, where it needs no password: on CI only, the one
    /// place `ci-packages` runs. The Tests job runs it (`cargo test
    /// --workspace`); elsewhere it returns at once.
    #[test]
    fn a_root_command_past_its_limit_is_stopped() {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return;
        }
        // A duration no other process uses, to find it by.
        let marker = "600.4242";
        let start = Instant::now();
        let (ended, _) =
            run_as_root("sleep", &[marker.to_owned()], Duration::from_secs(2)).unwrap();
        let took = start.elapsed();
        assert_eq!(ended, Ended::CutOff(Duration::from_secs(2)));
        assert!(took < Duration::from_secs(10), "stopped after {took:?}");
        assert!(
            !running(&["sleep", marker]),
            "the command outlived its limit"
        );
    }

    #[test]
    fn a_root_command_within_its_limit_reports_its_status() {
        if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
            return;
        }
        let (ended, output) = run_as_root(
            "sh",
            &["-c".to_owned(), "echo hello; exit 3".to_owned()],
            Duration::from_mins(1),
        )
        .unwrap();
        assert_eq!(ended, Ended::Exited(3));
        assert_eq!(output, "hello\n");
    }
}
