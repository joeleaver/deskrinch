//! What `cargo xtask compositor-tests` is asked to run, and the command
//! line of one run of it, which a repeat runs again and again.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::ValueEnum;

use super::renderers::Choice;
use crate::session::Compositor;

/// The subcommand's name, which a run's command line starts with.
pub(super) const SUBCOMMAND: &str = "compositor-tests";

/// What `cargo xtask compositor-tests` is asked to run.
#[derive(Debug, Clone, PartialEq, Eq, clap::Args)]
pub(crate) struct Args {
    /// The compositors to run in; all of them when none is given.
    #[arg(value_enum)]
    pub(crate) compositors: Vec<Compositor>,
    /// The renderers the suites that draw run with: `software`; `gpu`
    /// (failing where the machine has no GPU); or `all`, software and,
    /// where the machine has a GPU, the GPU. Under sway and labwc the GPU
    /// pass runs in a session of its own whose compositor renders on the
    /// GPU too (wlroots' pixman renderer, which every other session has,
    /// offers nothing the GPU renderer can present on). The run says which
    /// it chose, and why. A run without the GPU renderer uses no GPU and
    /// tells its sessions' clients so (`DESKRINCH_SESSION_GPU=none`), as on
    /// a machine without one.
    #[arg(long, value_enum, default_value = "all")]
    pub(crate) renderer: Choice,
    /// Run only the suite of this name, as the run prints it ("xtask
    /// desktop", "deskrinch-panel compositor", ...); repeat it for several.
    /// Every suite when none is given.
    #[arg(long = "suite", value_name = "NAME")]
    pub(crate) suites: Vec<String>,
    /// Run only this test of the one suite chosen, by its full name in its
    /// binary (`layers::a_window_is_above_it`), wherever the suite runs it.
    #[arg(long, value_name = "TEST", requires = "suites")]
    pub(crate) test: Option<String>,
    /// Run it all this many times, each run in fresh sessions with its
    /// output in a file of its own under `--into`; then say how many passed
    /// and how many failed, and fail if any did.
    #[arg(long, value_name = "N", requires = "into",
          value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) repeat: Option<u32>,
    /// Where the repeated runs' output goes: a directory, made if missing,
    /// and empty if not.
    #[arg(long, value_name = "DIR", requires = "repeat")]
    pub(crate) into: Option<PathBuf>,
    /// How long each repeated run may take, in seconds: an hour by default,
    /// far beyond a whole run's own length. A run that overruns it is
    /// interrupted (as Ctrl-C does, so that it ends its sessions), killed if
    /// it has not ended two minutes later, noted in its file and counted as
    /// failed; the repeat goes on.
    #[arg(long, value_name = "SECONDS", default_value_t = 3600, requires = "repeat",
          value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) run_timeout: u64,
}

impl Args {
    /// The command line of one run as asked, without the repeat: what
    /// `cargo xtask` runs it with.
    pub(super) fn once(&self) -> Vec<OsString> {
        let mut line: Vec<OsString> = vec![SUBCOMMAND.into(), "--renderer".into()];
        line.push(value_name(&self.renderer).into());
        for suite in &self.suites {
            line.extend(["--suite".into(), suite.into()]);
        }
        if let Some(test) = &self.test {
            line.extend(["--test".into(), test.into()]);
        }
        line.extend(self.compositors.iter().map(|c| value_name(c).into()));
        line
    }
}

/// The name `value` is written with on the command line.
fn value_name(value: &impl ValueEnum) -> String {
    value
        .to_possible_value()
        .map(|value| value.get_name().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(subcommand)]
        task: Task,
    }

    #[derive(Debug, clap::Subcommand)]
    enum Task {
        CompositorTests(Args),
    }

    fn parse(line: &[&str]) -> Result<Args, clap::Error> {
        let line = ["xtask", SUBCOMMAND].iter().chain(line);
        Cli::try_parse_from(line).map(|cli| match cli.task {
            Task::CompositorTests(args) => args,
        })
    }

    #[test]
    fn one_run_is_the_same_run_without_the_repeat() {
        let asked = parse(&[
            "--renderer",
            "software",
            "--suite",
            "xtask desktop",
            "--test",
            "edges::a_window_at_the_edge",
            "--repeat",
            "20",
            "--into",
            "/tmp/runs",
            "--run-timeout",
            "900",
            "kwin",
            "labwc",
        ])
        .unwrap();
        assert_eq!(asked.repeat, Some(20));
        assert_eq!(asked.run_timeout, 900);
        let once = asked.once();
        assert_eq!(once[0], SUBCOMMAND);
        let strings: Vec<&str> = once[1..].iter().map(|arg| arg.to_str().unwrap()).collect();
        let again = parse(&strings).unwrap();
        assert_eq!(
            again,
            Args {
                repeat: None,
                into: None,
                run_timeout: 3600,
                ..asked
            }
        );
        let plain = parse(&[]).unwrap();
        let strings: Vec<String> = plain.once()[1..]
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(strings, ["--renderer", "all"]);
    }

    #[test]
    fn a_test_needs_a_suite_and_a_repeat_a_directory() {
        assert!(parse(&["--test", "a"]).is_err());
        assert!(parse(&["--repeat", "3"]).is_err());
        assert!(parse(&["--into", "/tmp/runs"]).is_err());
        assert!(parse(&["--repeat", "0", "--into", "/tmp/runs"]).is_err());
        assert!(parse(&["--run-timeout", "60"]).is_err());
        let runs = ["--repeat", "2", "--into", "/tmp/runs"];
        assert_eq!(parse(&runs).unwrap().run_timeout, 3600, "an hour");
        assert!(parse(&[&runs[..], &["--run-timeout", "0"]].concat()).is_err());
    }
}
