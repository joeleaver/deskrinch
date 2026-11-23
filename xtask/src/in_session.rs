//! `cargo xtask in-session`: the session's side of a test that runs a shell
//! component in a nested session, does something to it once it has come
//! far enough, and reads how it ended. The tests run it as the command of
//! `cargo xtask session` (`xtask/tests/in_session/`).
//!
//! It is hidden from `--help` because it is the tests': it is not for
//! running by hand. Outside a nested session it refuses, before it starts
//! anything: there it would end the real desktop's compositor.
//!
//! What it does, in order:
//!
//! 1. starts the component (the command after its options) with its
//!    standard error in `DIR/stderr`, its standard output discarded and
//!    the session's environment;
//! 2. waits, within the patience, for what [`Until`] names: reading that
//!    file, the component's first frame of a kind (rinch's `RINCH_PERF`
//!    lines, which the caller turns on); or, on the session's bus, the
//!    name `--name` owned, and for [`Until::Notified`] then a notification
//!    sent and the frame that shows it ([`bus`]);
//! 3. writes how many bytes the component had written to its standard
//!    error to `DIR/acted`, then does what [`Then`] names (ends the
//!    compositor, or sends the component a signal that asks it to stop):
//!    what came before is what the component said before it was acted on;
//! 4. waits, within the patience, for the component to end, and writes its
//!    status to `DIR/status` as a shell would give it: its exit code, or
//!    128 and the signal's number when a signal ended it; for
//!    [`Until::Notified`], first the reasons its `NotificationClosed`
//!    signals gave for the notification it sent, one a line, to
//!    `DIR/closed`.
//!
//! A component that ends before that frame, never draws it, or does not
//! end afterwards, and an action that cannot be done, are errors, said on
//! standard error, and `DIR/status` is not written. On every error path
//! the component is killed and reaped first: nothing is left running.

use std::fs::{self, File};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

use clap::ValueEnum;
use deskrinch_test_client::Compositor;
use deskrinch_test_component::perf::{PerfLine, parse};
use deskrinch_test_wait::{Deadline, PATIENCE, POLL};
use rustix::process::{Pid, Signal, kill_process};

use crate::Outcome;

mod bus;

/// The variable `cargo xtask session` names its compositor's process id in.
use deskrinch_test_session::COMPOSITOR_PID_VARIABLE as COMPOSITOR_PID;

/// `cargo xtask in-session`'s arguments.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// The directory the component's standard error and its status are
    /// written in.
    #[arg(long, value_name = "DIR")]
    dir: PathBuf,
    /// What to wait for before acting.
    #[arg(long, value_enum)]
    until: Until,
    /// The bus name the component owns when it is ready, for `--until
    /// owned` and `--until notified`.
    #[arg(
        long,
        value_name = "NAME",
        required_if_eq_any([("until", "owned"), ("until", "notified")])
    )]
    name: Option<String>,
    /// For `--until owned`: a line the component writes once it is up
    /// (its UI built, its display connection in use), waited for after the
    /// name.
    #[arg(long, value_name = "TEXT", required_if_eq("until", "owned"))]
    said: Option<String>,
    /// What to do then.
    #[arg(long, value_enum)]
    then: Then,
    /// The component.
    component: PathBuf,
    /// Its arguments.
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    arguments: Vec<String>,
}

/// What to wait for before acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Until {
    /// A presented frame, of any of its windows: a component with a window
    /// shown.
    Presented,
    /// A prepared frame: a component that starts hidden.
    Prepared,
    /// Its bus name (`--name`) owned on the session's bus, then the line
    /// `--said` written: the notification daemon up, with nothing to show.
    Owned,
    /// Its bus name owned, then a notification sent to
    /// `org.freedesktop.Notifications`, then a presented frame: the
    /// notification daemon showing a popup.
    Notified,
}

impl Until {
    /// Whether `log` has the frame line this waits for; `false` for the
    /// waits that end on the bus.
    fn in_log(self, log: &str) -> bool {
        log.lines().any(|line| {
            matches!(
                (self, parse(line)),
                (
                    Self::Presented | Self::Notified,
                    Some(PerfLine::Frame { .. })
                ) | (Self::Prepared, Some(PerfLine::Prepared { .. }))
            )
        })
    }
}

/// What to do once the component has come that far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Then {
    /// End the session's compositor (`SIGTERM` to it): the component's
    /// display goes away.
    KillCompositor,
    /// Send the component `SIGTERM`: asked to stop, as a session or a unit
    /// stops it.
    #[value(name = "signal:TERM")]
    SignalTerm,
    /// Send the component `SIGINT`: asked to stop, as Ctrl-C asks.
    #[value(name = "signal:INT")]
    SignalInt,
}

/// Why `cargo xtask in-session` could not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub(crate) enum InSessionError {
    /// It was run outside a nested session.
    #[error("cargo xtask in-session runs only inside cargo xtask session")]
    NotNested,
    /// The component could not be started.
    #[error("could not start {}", component.display())]
    Start {
        /// The component.
        component: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The component ended before the frame waited for.
    #[error("the component ended ({status}) before its first {until:?} frame; it said:\n{said}")]
    EndedEarly {
        /// The frame waited for.
        until: Until,
        /// How it ended.
        status: ExitStatus,
        /// Its standard error.
        said: String,
    },
    /// The frame did not come within the patience.
    #[error("no {until:?} frame within {PATIENCE:?}; the component said:\n{said}")]
    NoFrame {
        /// The frame waited for.
        until: Until,
        /// Its standard error.
        said: String,
    },
    /// The bus name was not owned within the patience.
    #[error("{name} was not owned within {PATIENCE:?}; the component said:\n{said}")]
    NotOwned {
        /// The name.
        name: String,
        /// Its standard error.
        said: String,
    },
    /// The line `--said` names did not come within the patience.
    #[error("the component did not say {line:?} within {PATIENCE:?}; it said:\n{said}")]
    NotSaid {
        /// The line.
        line: String,
        /// Its standard error.
        said: String,
    },
    /// The session's bus could not be asked, or the notification not sent.
    #[error("the session's bus")]
    Bus(#[source] zbus::Error),
    /// The session gave no compositor process id.
    #[error("{COMPOSITOR_PID} is not a process id")]
    NoCompositor,
    /// The component's process id does not fit a `pid_t`.
    #[error("the component's process id is not a process id")]
    NoPid,
    /// The compositor or the component could not be signalled.
    #[error("could not send the signal")]
    Kill(#[source] std::io::Error),
    /// The component did not end within the patience after the action.
    #[error("the component did not end within {PATIENCE:?} of {then:?}; it said:\n{said}")]
    NotEnded {
        /// What was done.
        then: Then,
        /// Its standard error.
        said: String,
    },
    /// The component's status had neither an exit code nor a signal.
    #[error("the component ended with a status that is neither an exit code nor a signal: {0}")]
    NoStatus(ExitStatus),
    /// A file in the directory could not be written or read, or the
    /// component could not be waited for.
    #[error("could not {what}")]
    Io {
        /// What was being done.
        what: &'static str,
        /// Why.
        source: std::io::Error,
    },
}

/// Runs the component as `args` say (see the module's doc).
pub(crate) fn run(args: &Args) -> Result<Outcome, crate::error::Error> {
    if Compositor::nested().is_none() {
        return Err(InSessionError::NotNested.into());
    }
    Ok(step(args).map(|()| Outcome::Passed)?)
}

fn step(args: &Args) -> Result<(), InSessionError> {
    let stderr_path = args.dir.join("stderr");
    let stderr = File::create(&stderr_path).map_err(io("make the component's stderr file"))?;
    let mut component = Command::new(&args.component)
        .args(&args.arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|source| InSessionError::Start {
            component: args.component.clone(),
            source,
        })?;
    let sent = match wait_until(&mut component, args, &stderr_path) {
        Ok(sent) => sent,
        Err(error) => {
            end(&mut component);
            return Err(error);
        }
    };
    let acted = fs::metadata(&stderr_path)
        .map(|metadata| metadata.len())
        .and_then(|written| fs::write(args.dir.join("acted"), format!("{written}\n")));
    if let Err(source) = acted {
        end(&mut component);
        return Err(InSessionError::Io {
            what: "write how much the component had said when acted on",
            source,
        });
    }
    if let Err(error) = act(args.then, &component) {
        end(&mut component);
        return Err(error);
    }
    let Some(status) = wait_end(&mut component)? else {
        end(&mut component);
        return Err(InSessionError::NotEnded {
            then: args.then,
            said: said(&stderr_path),
        });
    };
    let status = shell_status(status).ok_or(InSessionError::NoStatus(status))?;
    if let Some(sent) = sent {
        let reasons = sent
            .closed()
            .iter()
            .fold(String::new(), |mut lines, reason| {
                lines.push_str(&reason.to_string());
                lines.push('\n');
                lines
            });
        fs::write(args.dir.join("closed"), reasons)
            .map_err(io("write how the notification sent was closed"))?;
    }
    fs::write(args.dir.join("status"), format!("{status}\n"))
        .map_err(io("write the component's status"))
}

/// Waits for what `args.until` names, the component logging to `log`; the
/// notification sent, for [`Until::Notified`].
fn wait_until(
    component: &mut Child,
    args: &Args,
    log: &Path,
) -> Result<Option<bus::Sent>, InSessionError> {
    let until = args.until;
    if matches!(until, Until::Presented | Until::Prepared) {
        return wait_frame(component, until, log).map(|()| None);
    }
    // `--name` is required for these by its argument's definition.
    let name = args.name.as_deref().unwrap_or_default();
    let connection = bus::connect().map_err(InSessionError::Bus)?;
    let deadline = Deadline::patience();
    while !bus::owned(&connection, name).map_err(InSessionError::Bus)? {
        if let Some(status) = component
            .try_wait()
            .map_err(io("look whether the component ended"))?
        {
            return Err(InSessionError::EndedEarly {
                until,
                status,
                said: said(log),
            });
        }
        if deadline.passed() {
            return Err(InSessionError::NotOwned {
                name: name.to_owned(),
                said: said(log),
            });
        }
        std::thread::sleep(POLL);
    }
    if until == Until::Notified {
        let sent = bus::notify(connection).map_err(InSessionError::Bus)?;
        return wait_frame(component, until, log).map(|()| Some(sent));
    }
    // `--said` is required for `owned` by its argument's definition.
    let line = args.said.as_deref().unwrap_or_default();
    while !said(log).contains(line) {
        if let Some(status) = component
            .try_wait()
            .map_err(io("look whether the component ended"))?
        {
            return Err(InSessionError::EndedEarly {
                until,
                status,
                said: said(log),
            });
        }
        if deadline.passed() {
            return Err(InSessionError::NotSaid {
                line: line.to_owned(),
                said: said(log),
            });
        }
        std::thread::sleep(POLL);
    }
    Ok(None)
}

/// Waits for the component's first `until` frame in its log at `log`.
fn wait_frame(component: &mut Child, until: Until, log: &Path) -> Result<(), InSessionError> {
    let deadline = Deadline::patience();
    loop {
        if until.in_log(&said(log)) {
            return Ok(());
        }
        if let Some(status) = component
            .try_wait()
            .map_err(io("look whether the component ended"))?
        {
            return Err(InSessionError::EndedEarly {
                until,
                status,
                said: said(log),
            });
        }
        if deadline.passed() {
            return Err(InSessionError::NoFrame {
                until,
                said: said(log),
            });
        }
        std::thread::sleep(POLL);
    }
}

/// Does `then` to `component` or its session.
fn act(then: Then, component: &Child) -> Result<(), InSessionError> {
    let (pid, signal) = match then {
        Then::KillCompositor => {
            let pid = std::env::var(COMPOSITOR_PID)
                .ok()
                .and_then(|pid| pid.parse().ok())
                .and_then(Pid::from_raw)
                .ok_or(InSessionError::NoCompositor)?;
            (pid, Signal::TERM)
        }
        Then::SignalTerm => (child_pid(component)?, Signal::TERM),
        Then::SignalInt => (child_pid(component)?, Signal::INT),
    };
    kill_process(pid, signal).map_err(|error| InSessionError::Kill(error.into()))
}

/// The component's process id.
fn child_pid(component: &Child) -> Result<Pid, InSessionError> {
    i32::try_from(component.id())
        .ok()
        .and_then(Pid::from_raw)
        .ok_or(InSessionError::NoPid)
}

/// The component's status once it has ended, or `None` if it has not
/// within the patience.
fn wait_end(component: &mut Child) -> Result<Option<ExitStatus>, InSessionError> {
    let deadline = Deadline::patience();
    loop {
        if let Some(status) = component
            .try_wait()
            .map_err(io("look whether the component ended"))?
        {
            return Ok(Some(status));
        }
        if deadline.passed() {
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

/// Kills and reaps the component, after a failure.
fn end(component: &mut Child) {
    // It may have ended by itself meanwhile.
    drop(component.kill());
    drop(component.wait());
}

/// What the component wrote to its standard error so far.
fn said(log: &Path) -> String {
    fs::read_to_string(log).unwrap_or_default()
}

/// `status` as a shell gives it in `$?`: the exit code, or 128 and the
/// number of the signal that ended the process. `None` for a status with
/// neither, which a process waited for does not have (a stopped one is not
/// reported by `wait` without `WUNTRACED`).
fn shell_status(status: ExitStatus) -> Option<i32> {
    status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
}

fn io(what: &'static str) -> impl FnOnce(std::io::Error) -> InSessionError {
    move |source| InSessionError::Io { what, source }
}

#[cfg(test)]
mod tests;
