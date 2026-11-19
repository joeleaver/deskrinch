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
//! - `ci-packages install` removes from the restored cache what apt no
//!   longer fetches, then runs `apt-get install --yes
//!   --no-install-recommends <packages>` with the cache's directory as apt's
//!   archives, fetching only what the cache lacks.
//!
//! apt runs as root, each run with apt's own retry and request timeouts
//! ([`APT_OPTIONS`]) and cut off after its limit ([`limit`]) by `timeout`
//! running as root ([`bounded`]; apt has no option for a name that does not
//! resolve). apt's own timeouts end a request that gets no answer; the
//! limits bound what they do not: a name that never resolves, which shows
//! at `update`, the first contact, and a mirror that answers but serves
//! slowly, which shows at `install`, where the files are. A run cut off
//! while still fetching is reported as a slow mirror, not an unreachable
//! one. What each run's end means is [`classify::classify`]'s, and is
//! reported as GitHub annotations. It installs packages with `sudo`, so it
//! runs only on CI (`GITHUB_ACTIONS=true`) and refuses everywhere else.

mod annotation;
mod bounded;
mod cache;
mod classify;
mod plan;
mod progress;

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use self::annotation::annotation;
use self::bounded::User;
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

/// How long `apt-get` may run for `step`, so that the whole package step
/// ends before GitHub ends the tightest job; the test
/// `the_whole_step_fits_the_tightest_job` holds the figures.
///
/// - `update` fetches about 3.5 MB of indexes, in 2 to 15 s on every night
///   seen, and is where a server that cannot be reached shows: two minutes.
/// - The listing reads what `update` fetched and fetches nothing: well
///   under a second; twenty.
/// - `install` fetches what the cache lacks: nothing once `main` has saved
///   the job's packages, all of them (4 to 115 MB) for a job whose cache is
///   gone. On 2026-11-18 the mirror served steadily slowly, with no fetch
///   stalled and nothing retried: 97 kB/s for labwc's 53.3 MB (about 9
///   minutes needed) and 72 kB/s for sway's 52.2 MB (about 12). Eight
///   minutes, what the tightest jobs leave once the cache's restore and
///   save are counted: on such a night a job with no cache fails as slow,
///   and the next run of `main` that gets its packages caches them.
fn limit(step: Step) -> Duration {
    match step {
        Step::Update => Duration::from_mins(2),
        Step::List => Duration::from_secs(20),
        Step::Install => Duration::from_mins(8),
    }
}

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
    cache::prepare()?;
    let Some(updated) = run_step(Step::Update, packages, Path::new(cache::DIRECTORY), None)? else {
        return Ok(Outcome::Failed);
    };
    // Listed against an empty directory, the listing is every file the
    // packages need, whatever is cached.
    let empty = tempfile::tempdir().map_err(|source| Error::Io {
        what: "make an empty archives directory".into(),
        source,
    })?;
    let Some(listed) = run_step(Step::List, packages, empty.path(), updated.sign.as_deref())?
    else {
        return Ok(Outcome::Failed);
    };
    let plan = Plan::new(packages, downloads(&listed.output)?);
    let key = plan.key(&ci_variable("ImageOS")?, &ci_variable("RUNNER_ARCH")?);
    println!(
        "Package cache key {}: the {} files apt fetches for {}, {}.",
        key.exact,
        plan.downloads.len(),
        plan.packages.join(" "),
        megabytes(plan.bytes())
    );
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
    let restored = cache::contents(directory)?;
    println!("Package cache restored: {}.", cache::summary(&restored));
    let stale = cache::prune(directory, &plan.files())?;
    if stale.is_empty() {
        println!("Package cache: nothing in it that apt no longer fetches.");
    } else {
        println!(
            "Package cache: removed {} files apt no longer fetches: {}. Left: {}.",
            stale.len(),
            stale.join(" "),
            cache::summary(&cache::contents(directory)?)
        );
    }
    if run_step(Step::Install, &plan.packages, directory, None)?.is_none() {
        return Ok(Outcome::Failed);
    }
    let kept = cache::contents(directory)?;
    let named = plan.files();
    println!(
        "apt kept its download sandbox (no \"unsandboxed\" notice). Package cache after the \
         install: {}, {} of the {} files the key names.",
        cache::summary(&kept),
        kept.keys()
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

/// Runs `step`, with `archives` as apt's archives directory, and reports
/// its verdict; `None` if the job is to stop. `update_sign` is what an
/// earlier `update` could not reach.
fn run_step(
    step: Step,
    packages: &[String],
    archives: &Path,
    update_sign: Option<&str>,
) -> Result<Option<Ran>, Error> {
    // The listing fetches and writes nothing: the job's user may run it.
    let user = if step.fetches() {
        User::Root
    } else {
        User::Job
    };
    let (ended, output) = bounded::run(
        user,
        "apt-get",
        &arguments(step, packages, archives),
        limit(step),
    )?;
    let verdict = classify(step, ended, &output, update_sign);
    if let Some(line) = annotation(&verdict) {
        println!("{line}");
    }
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
        | Verdict::Failed { .. } => None,
    })
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

/// The arguments of `apt-get` for `step`, with `archives` as apt's
/// archives directory.
fn arguments(step: Step, packages: &[String], archives: &Path) -> Vec<String> {
    let mut args = Vec::new();
    if step.fetches() {
        args.extend(APT_OPTIONS.iter().map(|&option| option.to_owned()));
    }
    let install = ["install", "--yes", "--no-install-recommends"].map(String::from);
    let archives = format!("Dir::Cache::Archives={}/", archives.display());
    match step {
        Step::Update => args.push("update".into()),
        Step::List => {
            args.extend(["-o".into(), archives]);
            args.extend(install);
            args.extend(["--print-uris", "-qq"].map(String::from));
            args.extend(packages.iter().cloned());
        }
        Step::Install => {
            // apt-get keeps what it fetches by default; the cache needs it.
            args.extend(["-o".into(), archives]);
            args.extend(["-o", "APT::Keep-Downloaded-Packages=true"].map(String::from));
            args.extend(install);
            args.extend(packages.iter().cloned());
        }
    }
    args
}

#[cfg(test)]
mod tests;
