//! `cargo xtask ci-packages`: installs the Ubuntu packages a CI job needs,
//! failing fast, and saying so, when the package servers cannot be reached.
//!
//! It runs `sudo apt-get update`, then `sudo apt-get install --yes
//! --no-install-recommends <packages>`, each with apt's own retry and
//! request timeouts ([`APT_OPTIONS`]) and each cut off after [`RUN_LIMIT`]
//! (apt has no option for a name that does not resolve). What each run's
//! end means is [`classify::classify`]'s, and is reported as GitHub
//! annotations. It installs packages with `sudo`, so it runs only on CI
//! (`GITHUB_ACTIONS=true`) and refuses everywhere else.

mod classify;

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use self::classify::{Ended, Step, Verdict, classify};
use crate::Outcome;
use crate::error::Error;

/// apt's own limits: each request gives up after 20 s without an answer and
/// is tried twice more.
const APT_OPTIONS: [&str; 6] = [
    "-o",
    "Acquire::Retries=2",
    "-o",
    "Acquire::http::Timeout=20",
    "-o",
    "Acquire::https::Timeout=20",
];

/// How long one `apt-get` run may take: the two bound the step at ten
/// minutes.
const RUN_LIMIT: Duration = Duration::from_mins(5);

/// How often a run is looked at while it goes on.
const POLL: Duration = Duration::from_millis(100);

/// The variable GitHub Actions sets to `true` in every job.
const CI_VARIABLE: &str = "GITHUB_ACTIONS";

/// Installs `packages`, on CI only.
pub(crate) fn run(packages: &[String]) -> Result<Outcome, Error> {
    on_ci(std::env::var(CI_VARIABLE).ok().as_deref())?;
    let mut update_sign = None;
    for step in [Step::Update, Step::Install] {
        let (ended, output) = apt(step, packages)?;
        match classify(step, ended, &output, update_sign.as_deref()) {
            Verdict::Done => {}
            Verdict::IndexNotUpdated(sign) => {
                println!("::warning title=Package index not updated::{sign}");
                update_sign = Some(sign);
            }
            Verdict::Unreachable(reason) => {
                println!(
                    "::error title=Package servers unreachable::apt could not reach Ubuntu's \
                     package servers ({reason}). This is the runner's network, not the code: \
                     re-run the job."
                );
                return Ok(Outcome::Failed);
            }
            Verdict::Failed { step, ended } => {
                println!(
                    "::error title=apt-get {step} failed::apt-get {step} ended with {ended:?}; \
                     see its output above."
                );
                return Ok(Outcome::Failed);
            }
        }
    }
    Ok(Outcome::Passed)
}

/// Whether [`CI_VARIABLE`]'s value, `value`, says this is a CI job.
fn on_ci(value: Option<&str>) -> Result<(), Error> {
    if value == Some("true") {
        Ok(())
    } else {
        Err(Error::NotInCi)
    }
}

/// The arguments of `sudo` for `step`.
fn arguments(step: Step, packages: &[String]) -> Vec<String> {
    let mut args = vec!["apt-get".to_owned()];
    args.extend(APT_OPTIONS.iter().map(|&option| option.to_owned()));
    match step {
        Step::Update => args.push("update".into()),
        Step::Install => {
            args.extend(["install", "--yes", "--no-install-recommends"].map(String::from));
            args.extend(packages.iter().cloned());
        }
    }
    args
}

/// Runs `sudo apt-get` for `step`, printing its output as it comes, and
/// returns how it ended and what it printed.
fn apt(step: Step, packages: &[String]) -> Result<(Ended, String), Error> {
    let mut child = Command::new("sudo")
        .args(arguments(step, packages))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: "sudo apt-get".into(),
            source,
        })?;
    let output = Arc::new(Mutex::new(String::new()));
    let readers = [
        child.stdout.take().map(|out| echo(out, &output)),
        child.stderr.take().map(|err| echo(err, &output)),
    ];
    let ended = wait(&mut child, RUN_LIMIT)?;
    for reader in readers.into_iter().flatten() {
        // A reader only copies lines; a panic in it already reported itself.
        let _joined = reader.join();
    }
    let output = output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    Ok((ended, output))
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

/// Waits for `child` for at most `limit`, then stops it (`sudo` passes the
/// signal on to apt).
fn wait(child: &mut Child, limit: Duration) -> Result<Ended, Error> {
    let deadline = Instant::now() + limit;
    loop {
        let status = child.try_wait().map_err(|source| Error::Io {
            what: "wait for apt-get".into(),
            source,
        })?;
        if let Some(status) = status {
            return Ok(ended(status));
        }
        if Instant::now() >= deadline {
            // Already gone, or not ours to signal: either way it is ending.
            let _ = kill_process(Pid::from_child(child), Signal::TERM);
            drop(child.wait());
            return Ok(Ended::CutOff(limit));
        }
        thread::sleep(POLL);
    }
}

/// How a run that ended with `status` ended.
fn ended(status: std::process::ExitStatus) -> Ended {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _) => Ended::Exited(code),
        (None, Some(signal)) => Ended::Signalled(signal),
        (None, None) => Ended::Exited(-1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_outside_ci() {
        for value in [None, Some(""), Some("false"), Some("1"), Some("TRUE")] {
            assert!(matches!(on_ci(value), Err(Error::NotInCi)), "{value:?}");
        }
        assert!(on_ci(Some("true")).is_ok());
    }

    #[test]
    fn installs_with_aptss_limits_and_no_recommends() {
        let packages = ["sway".to_owned(), "dbus-daemon".to_owned()];
        assert_eq!(
            arguments(Step::Install, &packages).join(" "),
            "apt-get -o Acquire::Retries=2 -o Acquire::http::Timeout=20 \
             -o Acquire::https::Timeout=20 install --yes --no-install-recommends sway dbus-daemon"
        );
        assert_eq!(arguments(Step::Update, &packages).last().unwrap(), "update");
    }

    #[test]
    fn a_run_past_its_limit_is_cut_off() {
        let mut child = Command::new("sleep").arg("600").spawn().unwrap();
        let ended = wait(&mut child, Duration::from_millis(200)).unwrap();
        assert_eq!(ended, Ended::CutOff(Duration::from_millis(200)));
        assert!(child.try_wait().unwrap().is_some(), "it was stopped");
    }

    #[test]
    fn a_run_within_its_limit_reports_its_status() {
        let mut child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        assert_eq!(wait(&mut child, RUN_LIMIT).unwrap(), Ended::Exited(3));
    }
}
