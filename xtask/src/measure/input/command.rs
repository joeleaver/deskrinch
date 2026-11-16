//! `--step-command 'CMD'`: another program's command as a measured step, a
//! step like a key or a pointer move ([`super::steps`]).
//!
//! Each command is run as a client of the session through `sh -c`, with
//! the session's environment, after the idle window, in the order given:
//! sending the program a notification, or copying a second configuration
//! over the one `--config` seeded. Its own process is not in the figure:
//! the meter reads the measured program alone. A command that ends with a
//! status other than 0, or does not end within the first-frame timeout,
//! fails the run with its output.
//!
//! A command step is measured from just before the command starts to
//! settled. Its frames are every window's, since its effect may be on any
//! window or open or close one (a notification's popup), and the windows it
//! opened and closed are noted in the run. It must lead to a frame on some
//! window or to a window opened or closed: a step after which the program
//! showed nothing measured nothing. A command that only closes a window
//! (a popup's close) shows its change by that window's closing, frame or
//! none.

use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::super::report::Run;
use super::super::settle::Running;
use super::steps::{Measured, Rows, Scope, measure_steps};
use super::{MeasuredInputError, Meter};
use crate::error::Error;
use crate::session::interrupt;

/// The commands of `--step-command`, and how to start a shell in the
/// session.
pub(crate) struct Commands<'a> {
    /// The commands, in order.
    pub(crate) list: &'a [String],
    /// `sh` as a client of the session.
    pub(crate) shell: &'a dyn Fn() -> Command,
}

/// Runs each command as a measured step (see the module's doc), with what
/// `meter` measures.
pub(crate) fn run_commands(
    run: &mut Run,
    running: &mut Running<'_>,
    commands: &Commands<'_>,
    meter: &mut dyn Meter,
) -> Result<(), Error> {
    let count = commands.list.len();
    let steps: Vec<Measured<String>> = commands
        .list
        .iter()
        .enumerate()
        .map(|(index, command)| Measured {
            action: command.clone(),
            row: format!("command step {} of {count}", index + 1),
            named: format!("command step {} of {count} ({command:?})", index + 1),
            dump: format!("command-{}", index + 1),
        })
        .collect();
    let rows = Rows {
        prefix: "",
        during: "",
        from: "command",
        sum: "commands, the steps' sum",
        frames: "commands: frames presented",
        scope: Scope::Every,
    };
    let timeout = running.timeout;
    measure_steps(
        run,
        running,
        &mut |command: &String| run_one(commands.shell, command, timeout),
        &steps,
        &rows,
        meter,
    )
}

/// Runs `command` through `shell`, waiting up to `timeout` for it to end.
fn run_one(shell: &dyn Fn() -> Command, command: &str, timeout: Duration) -> Result<(), Error> {
    let mut child = shell()
        .args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: "sh".to_owned(),
            source,
        })?;
    let readers = [
        child.stdout.take().map(read_all),
        child.stderr.take().map(read_all),
    ];
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|source| Error::Io {
            what: format!("wait for the step's command {command:?}"),
            source,
        })? {
            break status;
        }
        if start.elapsed() >= timeout {
            drop(child.kill());
            drop(child.wait());
            return Err(MeasuredInputError::CommandHung {
                command: command.to_owned(),
                waited: timeout,
            }
            .into());
        }
        interrupt::check()?;
        thread::sleep(Duration::from_millis(20));
    };
    let output: String = readers
        .into_iter()
        .flatten()
        .map(|reader| reader.join().unwrap_or_default())
        .collect();
    check(command, status, output)
}

/// What `command`, ended with `status`, means for the run.
fn check(command: &str, status: ExitStatus, output: String) -> Result<(), Error> {
    if status.success() {
        Ok(())
    } else {
        Err(MeasuredInputError::CommandFailed {
            command: command.to_owned(),
            status,
            output,
        }
        .into())
    }
}

/// Reads all of `pipe` on a thread of its own, so that a command that
/// writes more than a pipe holds does not wait on it.
fn read_all(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        drop(pipe.read_to_string(&mut text));
        text
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    fn sh() -> Command {
        Command::new("sh")
    }

    #[test]
    fn a_command_that_ends_with_0_passes_and_one_that_does_not_fails_with_its_output() {
        assert!(run_one(&sh, "true", Duration::from_secs(10)).is_ok());
        let error = run_one(&sh, "echo said; exit 3", Duration::from_secs(10))
            .unwrap_err()
            .to_string();
        assert!(error.contains("exit status: 3"), "{error}");
        assert!(error.contains("said"), "{error}");
        assert!(check("x", ExitStatus::from_raw(0), String::new()).is_ok());
    }

    #[test]
    fn a_command_that_does_not_end_in_time_fails_and_is_ended() {
        let error = run_one(&sh, "exec sleep 30", Duration::from_millis(300))
            .unwrap_err()
            .to_string();
        assert!(error.contains("did not end within"), "{error}");
    }
}
