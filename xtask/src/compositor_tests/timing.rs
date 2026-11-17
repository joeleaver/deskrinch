//! How long a run took, printed at its end: each suite run's time (a
//! suite's binary in one session, with one renderer), and the slowest tests
//! of the run, so that what a suite costs is in every run's output. It is
//! never part of the pass or fail.
//!
//! A test's time is read from its binary's lines as they come: the tests
//! run one at a time, and libtest prints `test NAME ... ok` (or `FAILED`)
//! as each ends, so a test's time runs from the line before it (the
//! previous test's, or `running N tests`) to its own.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// How many tests the end of a run names as its slowest.
const SLOWEST: usize = 10;

/// One suite run: what it was, how long it took, and each of its tests'
/// times, in the order they ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SuiteRun {
    pub(super) name: String,
    pub(super) took: Duration,
    pub(super) tests: Vec<(String, Duration)>,
}

/// Times one suite run from its binary's lines.
#[derive(Debug)]
pub(super) struct Timer {
    started: Instant,
    last: Instant,
    tests: Vec<(String, Duration)>,
}

impl Timer {
    /// A suite run starting now.
    pub(super) fn start() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            tests: Vec::new(),
        }
    }

    /// A line the binary printed, read at `at`.
    pub(super) fn line(&mut self, at: Instant, line: &str) {
        if line.starts_with("running ") {
            // The binary has started and listed its tests: the first test's
            // time runs from here.
            self.last = at;
        } else if let Some(name) = finished(line) {
            self.tests
                .push((name.to_owned(), at.saturating_duration_since(self.last)));
            self.last = at;
        }
    }

    /// The suite run, named `name`, ended now.
    pub(super) fn finish(self, name: String) -> SuiteRun {
        SuiteRun {
            name,
            took: self.started.elapsed(),
            tests: self.tests,
        }
    }
}

/// The name of the test whose end `line` reports, if it does:
/// `test NAME ... ok` or `... FAILED`.
fn finished(line: &str) -> Option<&str> {
    let (name, result) = line.strip_prefix("test ")?.split_once(" ... ")?;
    matches!(result.trim_end(), "ok" | "FAILED").then_some(name)
}

/// The end of a run's output: each suite run's time and the run's, then
/// its slowest tests.
pub(super) fn summary(runs: &[SuiteRun]) -> String {
    let mut out =
        String::from("compositor-tests: time per suite run (not part of the pass or fail):\n");
    for run in runs {
        let _ = writeln!(out, "  {:>8}  {}", seconds(run.took), run.name);
    }
    let total: Duration = runs.iter().map(|run| run.took).sum();
    let _ = writeln!(out, "  {:>8}  in all", seconds(total));
    let mut tests: Vec<(&str, &str, Duration)> = runs
        .iter()
        .flat_map(|run| {
            run.tests
                .iter()
                .map(|(test, took)| (test.as_str(), run.name.as_str(), *took))
        })
        .collect();
    tests.sort_by_key(|&(_, _, took)| std::cmp::Reverse(took));
    let _ = writeln!(out, "compositor-tests: the slowest tests of the run:");
    for (test, run, took) in tests.into_iter().take(SLOWEST) {
        let _ = writeln!(out, "  {:>8}  {test}  ({run})", seconds(took));
    }
    out
}

/// `took` in seconds, with a tenth.
fn seconds(took: Duration) -> String {
    format!("{:.1} s", took.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_is_timed_from_the_line_before_its_own() {
        let mut timer = Timer::start();
        let start = timer.started;
        let at = |ms| start + Duration::from_millis(ms);
        timer.line(at(100), "running 3 tests");
        timer.line(at(1_100), "test room::fills ... ok");
        timer.line(at(1_200), "some output of the test");
        timer.line(at(4_100), "test frames::one ... FAILED");
        timer.line(at(4_200), "test result: FAILED. 1 passed; 1 failed");
        let run = timer.finish("x".to_owned());
        assert_eq!(
            run.tests,
            [
                ("room::fills".to_owned(), Duration::from_secs(1)),
                ("frames::one".to_owned(), Duration::from_secs(3)),
            ]
        );
    }

    #[test]
    fn only_a_test_s_end_is_a_test() {
        assert_eq!(finished("test a::b ... ok"), Some("a::b"));
        assert_eq!(finished("test a::b ... FAILED"), Some("a::b"));
        assert_eq!(finished("test result: ok. 3 passed"), None);
        assert_eq!(finished("running 3 tests"), None);
        assert_eq!(finished("test a::b ... ignored, needs a compositor"), None);
    }

    #[test]
    fn the_summary_gives_each_suite_run_and_the_slowest_tests_first() {
        let runs = [
            SuiteRun {
                name: "notifications in sway".to_owned(),
                took: Duration::from_secs(361),
                tests: vec![
                    ("room::shrinks".to_owned(), Duration::from_millis(45_500)),
                    ("shown::one".to_owned(), Duration::from_millis(3_000)),
                ],
            },
            SuiteRun {
                name: "panel in sway".to_owned(),
                took: Duration::from_secs(72),
                tests: vec![("frames::idle".to_owned(), Duration::from_secs(9))],
            },
        ];
        let summary = summary(&runs);
        assert!(
            summary.contains("   361.0 s  notifications in sway\n"),
            "{summary}"
        );
        assert!(summary.contains("   433.0 s  in all\n"), "{summary}");
        let slow = summary
            .find("45.5 s  room::shrinks  (notifications in sway)")
            .unwrap();
        let next = summary
            .find("9.0 s  frames::idle  (panel in sway)")
            .unwrap();
        assert!(slow < next, "{summary}");
        assert!(summary.contains("not part of the pass or fail"));
    }
}
