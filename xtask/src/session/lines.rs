//! A test binary's standard output, line by line as it comes, each with
//! when it was read: what the run prints, and what it times its tests by.
//!
//! The lines are read on a thread of their own, and the binary's end is
//! waited for here. Something a test started may hold the pipe open after
//! the binary has exited (a program left running until the session ends):
//! so once the binary has exited, the lines still to come are waited for
//! [`DRAIN`] at most, and the reader is then left to end with the pipe.

use std::io::{self, BufRead, BufReader};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// How long, after the binary has exited, the lines still in its pipe are
/// waited for.
const DRAIN: Duration = Duration::from_secs(2);
/// How often the binary's end is looked at while no line comes.
const POLL: Duration = Duration::from_millis(50);

/// Runs `command` with its standard output piped, handing each line to
/// `on_line` with when it was read, and returns how the command ended.
pub(super) fn run_with_lines(
    command: &mut Command,
    on_line: &mut dyn FnMut(Instant, &str),
) -> io::Result<ExitStatus> {
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("no stdout"))?;
    let (sender, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if sender.send((Instant::now(), line)).is_err() {
                break;
            }
        }
    });
    let status = loop {
        match lines.recv_timeout(POLL) {
            Ok((at, line)) => on_line(at, &line),
            Err(RecvTimeoutError::Timeout) => {}
            // The pipe has closed: only the binary's end is left.
            Err(RecvTimeoutError::Disconnected) => break child.wait()?,
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
    };
    let deadline = Instant::now() + DRAIN;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match lines.recv_timeout(left) {
            Ok((at, line)) => on_line(at, &line),
            Err(_) => break,
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_is_handed_on_in_order_with_when_it_came() {
        let mut seen = Vec::new();
        let started = Instant::now();
        let status = run_with_lines(
            Command::new("sh").args(["-c", "echo one; sleep 0.3; echo two; exit 3"]),
            &mut |at, line| seen.push((at, line.to_owned())),
        )
        .unwrap();
        assert_eq!(status.code(), Some(3));
        let lines: Vec<&str> = seen.iter().map(|(_, line)| line.as_str()).collect();
        assert_eq!(lines, ["one", "two"]);
        assert!(seen[0].0 >= started);
        assert!(seen[1].0.duration_since(seen[0].0) >= Duration::from_millis(250));
    }

    #[test]
    fn a_pipe_held_open_after_the_end_is_not_waited_for_past_the_drain() {
        let started = Instant::now();
        let mut lines = 0;
        // The binary ends at once; what it started holds the pipe 30 s.
        let status = run_with_lines(
            Command::new("sh").args(["-c", "echo ended; sleep 30 &"]),
            &mut |_, _| lines += 1,
        )
        .unwrap();
        assert!(status.success());
        assert_eq!(lines, 1);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
