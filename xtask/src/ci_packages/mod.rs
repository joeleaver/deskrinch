//! `cargo xtask ci-packages`: installs the Ubuntu packages a CI job needs,
//! failing fast, and saying so, when the package servers cannot be reached.
//!
//! It runs `apt-get update`, then `apt-get install --yes
//! --no-install-recommends <packages>`, as root, each with apt's own retry
//! and request timeouts ([`APT_OPTIONS`]) and each cut off after
//! [`RUN_LIMIT`] by `timeout` running as root ([`bounded`]; apt has no
//! option for a name that does not resolve). What each run's
//! end means is [`classify::classify`]'s, and is reported as GitHub
//! annotations. It installs packages with `sudo`, so it runs only on CI
//! (`GITHUB_ACTIONS=true`) and refuses everywhere else.

mod bounded;
mod classify;

use std::time::Duration;

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
                    "::error title=apt-get {step} failed::apt-get {step} {ended}; see its \
                     output above."
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

/// The arguments of `apt-get` for `step`.
fn arguments(step: Step, packages: &[String]) -> Vec<String> {
    let mut args = Vec::new();
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

/// Runs `apt-get` for `step` as root, within [`RUN_LIMIT`].
fn apt(step: Step, packages: &[String]) -> Result<(Ended, String), Error> {
    bounded::run_as_root("apt-get", &arguments(step, packages), RUN_LIMIT)
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
    fn installs_with_apts_limits_and_no_recommends() {
        let packages = ["sway".to_owned(), "dbus-daemon".to_owned()];
        assert_eq!(
            arguments(Step::Install, &packages).join(" "),
            "-o Acquire::Retries=2 -o Acquire::http::Timeout=20 \
             -o Acquire::https::Timeout=20 install --yes --no-install-recommends sway dbus-daemon"
        );
        assert_eq!(arguments(Step::Update, &packages).last().unwrap(), "update");
    }
}
