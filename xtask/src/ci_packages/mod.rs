//! `cargo xtask ci-packages`: installs the Ubuntu packages a CI job needs,
//! from GitHub's cache where it can, failing fast, and saying so, when the
//! package servers cannot be reached.
//!
//! It runs in two steps, with the cache restored between them (the
//! `apt-packages` action):
//!
//! - `ci-packages update <packages>` makes the cache's directory
//!   ([`cache`]), runs `apt-get update`, lists what `apt-get install` would
//!   fetch with nothing cached (`--print-uris`), and from that names the
//!   cache ([`plan::Plan::key`]). It writes the directory, the key, the key's
//!   prefix and the number of files to `$GITHUB_OUTPUT`, and the plan to
//!   `$RUNNER_TEMP` for the next step.
//! - `ci-packages install` holds every restored file to the plan's size
//!   and checksum and removes what does not match ([`check`]), then runs
//!   `apt-get install --yes --no-install-recommends <packages>` with the
//!   cache's directory as apt's archives, fetching only what the cache
//!   lacks.
//!
//! apt runs as root, each run with apt's own retry and request timeouts
//! ([`APT_OPTIONS`]) and cut off after its limit, its share of the step's
//! one budget ([`budget`]), by `timeout` running as root ([`bounded`]; apt
//! has no option for a name that does not resolve). apt's own timeouts end
//! a request that gets no answer; the limits bound what they do not: a
//! name that never resolves, which shows
//! at `update`, the first contact, and a mirror that answers but serves
//! slowly, which shows at `install`, where the files are. A run cut off
//! while still fetching is reported as a slow mirror, not an unreachable
//! one. What each run's end means is [`classify::classify`]'s, and is
//! reported as GitHub annotations. It installs packages with `sudo`, so it
//! runs only on CI (`GITHUB_ACTIONS=true`) and refuses everywhere else.

mod annotation;
mod bounded;
mod budget;
mod cache;
mod check;
mod classify;
mod plan;
mod progress;

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use self::annotation::annotation;
use self::bounded::User;
use self::budget::Limit;
use self::classify::{Step, Verdict, classify};
use self::plan::{Plan, downloads, megabytes};
use crate::Outcome;
use crate::error::Error;

/// The two steps, each its own command.
#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Make the package cache's directory, update apt's index, and name the
    /// cache after the files apt would fetch (written to `$GITHUB_OUTPUT`).
    Update {
        /// The packages.
        #[arg(required = true)]
        packages: Vec<String>,
    },
    /// Install the packages `update` planned, fetching what the restored
    /// cache lacks.
    Install,
}

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

/// The variable GitHub Actions sets to `true` in every job.
const CI_VARIABLE: &str = "GITHUB_ACTIONS";

/// The plan's file name in `$RUNNER_TEMP`, the job's own directory.
const PLAN_FILE: &str = "deskrinch-apt-plan.json";

/// Runs `command`, on CI only.
pub(crate) fn run(command: &Command) -> Result<Outcome, Error> {
    on_ci(std::env::var(CI_VARIABLE).ok().as_deref())?;
    match command {
        Command::Update { packages } => update(packages),
        Command::Install => install(),
    }
}

/// `ci-packages update`.
fn update(packages: &[String]) -> Result<Outcome, Error> {
    let started = SystemTime::now();
    cache::prepare()?;
    let Some(updated) = run_step(
        Step::Update,
        packages,
        Path::new(cache::DIRECTORY),
        None,
        started,
    )?
    else {
        return Ok(Outcome::Failed);
    };
    let scratch = listing_directory(Path::new(EXTENDED_STATES))?;
    let Some(listed) = run_step(
        Step::List,
        packages,
        scratch.path(),
        updated.sign.as_deref(),
        started,
    )?
    else {
        return Ok(Outcome::Failed);
    };
    let plan = Plan::new(started, packages, downloads(&listed.output)?);
    let key = plan.key(&ci_variable("ImageOS")?, &ci_variable("RUNNER_ARCH")?);
    let kinds: Vec<String> = plan
        .kinds()
        .into_iter()
        .map(|(kind, files)| format!("{files} {kind}"))
        .collect();
    println!(
        "Package cache key {}: the {} files apt fetches for {}, {}; their checksums: {}.",
        key.exact,
        plan.downloads.len(),
        plan.packages.join(" "),
        megabytes(plan.bytes()),
        if kinds.is_empty() {
            "none".to_owned()
        } else {
            kinds.join(", ")
        }
    );
    let unverifiable = plan.unverifiable();
    if let Some(first) = unverifiable.first() {
        report(&Verdict::Unverifiable {
            files: unverifiable.len(),
            first: format!("{} {}", first.file, first.checksum),
        });
        return Ok(Outcome::Failed);
    }
    plan.save(&plan_path()?)?;
    let files = plan.downloads.len().to_string();
    write_outputs(
        Path::new(&ci_variable("GITHUB_OUTPUT")?),
        &[
            ("directory", cache::DIRECTORY),
            ("key", &key.exact),
            ("restore-key", &key.prefix),
            ("files", &files),
        ],
    )?;
    Ok(Outcome::Passed)
}

/// `ci-packages install`.
fn install() -> Result<Outcome, Error> {
    let plan = Plan::load(&plan_path()?)?;
    let directory = Path::new(cache::DIRECTORY);
    let checked = check::check(directory, &plan.downloads)?;
    println!(
        "Package cache restored: {} files; {} match the index's size and checksum.",
        checked.kept.len() + checked.removed.len(),
        cache::summary(&checked.kept)
    );
    for (file, unfit) in &checked.removed {
        println!("Package cache: removed {file}: {unfit}.");
    }
    if run_step(Step::Install, &plan.packages, directory, None, plan.started)?.is_none() {
        return Ok(Outcome::Failed);
    }
    let after = cache::contents(directory)?;
    let fetched: BTreeMap<String, u64> = after
        .iter()
        .filter(|(file, _)| !checked.kept.contains_key(*file))
        .map(|(file, size)| (file.clone(), *size))
        .collect();
    println!(
        "Packages from the cache: {}; fetched: {}.",
        cache::summary(&checked.kept),
        cache::summary(&fetched)
    );
    if !fetched.is_empty() {
        println!(
            "apt fetched them in its download sandbox: its output has no \"unsandboxed\" \
             notice."
        );
    }
    let named = plan.files();
    println!(
        "Package cache after the install: {}, {} of the {} files the key names.",
        cache::summary(&after),
        after
            .keys()
            .filter(|file| named.contains(file.as_str()))
            .count(),
        named.len()
    );
    Ok(Outcome::Passed)
}

/// A run of `apt-get` after which the job goes on.
struct Ran {
    /// What an `update` could not reach, if anything.
    sign: Option<String>,
    /// What it printed.
    output: String,
}

/// Runs `step` in `dir` (see [`arguments`]), within its share of the budget
/// of a step that started at `started`, and reports its verdict; `None` if
/// the job is to stop. `update_sign` is what an earlier `update` could not
/// reach.
fn run_step(
    step: Step,
    packages: &[String],
    dir: &Path,
    update_sign: Option<&str>,
    started: SystemTime,
) -> Result<Option<Ran>, Error> {
    let now = SystemTime::now();
    let limit = match budget::limit(step, started, now) {
        Limit::Run(limit) => limit,
        Limit::NoTimeLeft { used, left, floor } => {
            report(&Verdict::NoTimeLeft {
                step,
                used,
                left,
                floor,
            });
            return Ok(None);
        }
    };
    println!(
        "apt-get {step}: given {}s; the package step has used {}s of its {}s.",
        limit.as_secs(),
        now.duration_since(started).unwrap_or_default().as_secs(),
        budget::BUDGET.as_secs()
    );
    // The listing fetches nothing and writes only in its own directory: the
    // job's user may run it.
    let user = if step.fetches() {
        User::Root
    } else {
        User::Job
    };
    let began = Instant::now();
    let (ended, output) = bounded::run(user, "apt-get", &arguments(step, packages, dir), limit)?;
    println!("apt-get {step} ran {}s.", began.elapsed().as_secs());
    let verdict = classify(step, ended, &output, update_sign);
    report(&verdict);
    Ok(match verdict {
        Verdict::Done => Some(Ran { sign: None, output }),
        Verdict::IndexNotUpdated(sign) => Some(Ran {
            sign: Some(sign),
            output,
        }),
        Verdict::Unreachable(_)
        | Verdict::Slow(_)
        | Verdict::CutOffAfterDownload { .. }
        | Verdict::Unsandboxed(_)
        | Verdict::Unverifiable { .. }
        | Verdict::NoTimeLeft { .. }
        | Verdict::Failed { .. } => None,
    })
}

/// Prints `verdict`'s annotation, if it has one.
fn report(verdict: &Verdict) {
    if let Some(line) = annotation(verdict) {
        println!("{line}");
    }
}

/// The kind of checksum the listing asks apt for: one the check verifies.
const LISTED_CHECKSUM: &str = "SHA256";

/// apt's record of which packages were installed only as another's
/// dependency.
const EXTENDED_STATES: &str = "/var/lib/apt/extended_states";

/// The listing's own directory: an empty `archives`, so that it lists
/// every file the packages need, whatever is cached, and a copy of
/// `extended_states` (apt's record, [`EXTENDED_STATES`], when there is
/// one). `apt-get install` rewrites that record even when it only lists,
/// to mark a named package that was installed as a dependency (the
/// runner's `dbus-user-session`) as installed by name, and only root may
/// write apt's own.
fn listing_directory(extended_states: &Path) -> Result<tempfile::TempDir, Error> {
    let io = |what: String| move |source| Error::Io { what, source };
    let dir = tempfile::tempdir().map_err(io("make the listing's directory".into()))?;
    let archives = dir.path().join("archives");
    std::fs::create_dir(&archives).map_err(io(format!("make {}", archives.display())))?;
    match std::fs::copy(extended_states, dir.path().join("extended_states")) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(io(format!("copy {}", extended_states.display()))(error));
        }
        _ => {}
    }
    Ok(dir)
}

/// Whether [`CI_VARIABLE`]'s value, `value`, says this is a CI job.
fn on_ci(value: Option<&str>) -> Result<(), Error> {
    if value == Some("true") {
        Ok(())
    } else {
        Err(Error::NotInCi)
    }
}

/// The value of `name`, a variable GitHub Actions sets in every job.
fn ci_variable(name: &'static str) -> Result<String, Error> {
    std::env::var(name).map_err(|_| Error::CiVariable { name })
}

/// Where `update` leaves the plan for `install`.
fn plan_path() -> Result<PathBuf, Error> {
    Ok(Path::new(&ci_variable("RUNNER_TEMP")?).join(PLAN_FILE))
}

/// Appends `outputs` to the step's output file, `path`, as `name=value`
/// lines.
fn write_outputs(path: &Path, outputs: &[(&str, &str)]) -> Result<(), Error> {
    let io = |source| Error::Io {
        what: format!("write the step's outputs to {}", path.display()),
        source,
    };
    let mut file = OpenOptions::new().append(true).open(path).map_err(io)?;
    for (name, value) in outputs {
        writeln!(file, "{name}={value}").map_err(io)?;
    }
    Ok(())
}

/// The arguments of `apt-get` for `step`, in `dir`: apt's archives
/// directory for `update` and `install`, and for the listing its own
/// directory ([`listing_directory`]).
fn arguments(step: Step, packages: &[String], dir: &Path) -> Vec<String> {
    let mut args = Vec::new();
    if step.fetches() {
        args.extend(APT_OPTIONS.iter().map(|&option| option.to_owned()));
    }
    let install = ["install", "--yes", "--no-install-recommends"].map(String::from);
    let archives = |archives: &Path| format!("Dir::Cache::Archives={}/", archives.display());
    match step {
        Step::Update => args.push("update".into()),
        Step::List => {
            // Unless told which, apt prints the first of a file's checksums
            // in its own order of kinds (`HashStringList::find`): MD5Sum,
            // which the check cannot verify.
            args.extend(["-o".into(), format!("Acquire::ForceHash={LISTED_CHECKSUM}")]);
            args.extend(["-o".into(), archives(&dir.join("archives"))]);
            args.extend([
                "-o".into(),
                format!(
                    "Dir::State::extended_states={}",
                    dir.join("extended_states").display()
                ),
            ]);
            args.extend(install);
            args.extend(["--print-uris", "-qq"].map(String::from));
            args.extend(packages.iter().cloned());
        }
        Step::Install => {
            // apt-get keeps what it fetches by default; the cache needs it.
            args.extend(["-o".into(), archives(dir)]);
            args.extend(["-o", "APT::Keep-Downloaded-Packages=true"].map(String::from));
            args.extend(install);
            args.extend(packages.iter().cloned());
        }
    }
    args
}

#[cfg(test)]
mod tests;
