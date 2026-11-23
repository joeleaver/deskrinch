//! Which suites a run runs, and which of their tests: `--suite` and
//! `--test`.
//!
//! A suite is named as the run prints it ([`Target::name`]): "xtask
//! desktop", "deskrinch-panel compositor", "deskrinch-wayland-client
//! compositor on libwayland". A name chooses every row of [`SUITES`] with
//! that name, each under its own compositors and passes with its own
//! filter (the workspaces' rows split their tests by compositor). A test
//! is named by its full path in its binary (`layers::a_window_is_above_it`)
//! and runs, exactly, in the rows whose filters let it run.
//!
//! [`Target::name`]: super::build::Target::name

use std::collections::BTreeSet;

use super::suites::{SUITES, Suite};
use crate::session::Compositor;

/// Why a selection cannot run.
#[derive(Debug, thiserror::Error)]
pub(crate) enum SelectionError {
    /// A `--suite` that no suite is named.
    #[error("no suite is named {name:?}; the suites are:\n  {}", names.join("\n  "))]
    UnknownSuite { name: String, names: Vec<String> },
    /// `--test` with other than one suite chosen.
    #[error("--test names a test of one suite: choose that suite alone with --suite")]
    TestNeedsOneSuite,
    /// A `--test` that the chosen suite does not have.
    #[error("{suite} has no compositor test named {test:?}; its tests are:\n  {}", tests.join("\n  "))]
    UnknownTest {
        suite: String,
        test: String,
        tests: Vec<String>,
    },
    /// A `--test` that every row of the chosen suite leaves out by its
    /// filter, under whichever compositor.
    #[error("{suite}'s filter leaves {test} out; its rows' filters are:\n  {}", filters.join("\n  "))]
    LeftOutByFilter {
        suite: String,
        test: String,
        /// Each row's filter, its words joined by spaces.
        filters: Vec<String>,
    },
    /// A selection whose rows run under none of the compositors asked for.
    #[error("{what} runs under {theirs}, none of {asked}")]
    OtherCompositors {
        what: String,
        /// The compositors its rows run under.
        theirs: String,
        asked: String,
    },
}

/// The suites a run runs, by their rows in [`SUITES`], and the one test of
/// theirs it runs, if one is named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Selection {
    rows: Vec<usize>,
    /// The names asked for; empty for every suite.
    asked: Vec<String>,
    test: Option<String>,
}

impl Selection {
    /// The suites named `names` (every suite when empty), and `test` of
    /// theirs when given, which needs exactly one suite named.
    pub(super) fn new(names: &[String], test: Option<&str>) -> Result<Self, SelectionError> {
        Self::from_table(&SUITES, names, test)
    }

    fn from_table(
        table: &[Suite],
        names: &[String],
        test: Option<&str>,
    ) -> Result<Self, SelectionError> {
        if test.is_some() && names.len() != 1 {
            return Err(SelectionError::TestNeedsOneSuite);
        }
        let known = suite_names(table);
        if let Some(name) = names.iter().find(|name| !known.contains(name)) {
            return Err(SelectionError::UnknownSuite {
                name: name.clone(),
                names: known,
            });
        }
        let rows = (0..table.len())
            .filter(|&row| names.is_empty() || names.contains(&table[row].target.name()))
            .collect();
        Ok(Self {
            rows,
            asked: names.to_vec(),
            test: test.map(str::to_owned),
        })
    }

    /// The rows of [`SUITES`] with the names chosen, in its order: the
    /// binaries to build.
    pub(super) fn rows(&self) -> &[usize] {
        &self.rows
    }

    /// The test asked for, if any.
    pub(super) fn test(&self) -> Option<&str> {
        self.test.as_deref()
    }

    /// The libtest arguments `suite`, a chosen row, runs with: its own
    /// filter, or the test asked for, exactly; then those of its
    /// [`Output`](super::suites::Output). `None` when its filter leaves
    /// that test out, and the row does not run.
    pub(super) fn args<'a>(&'a self, suite: &'a Suite) -> Option<Vec<&'a str>> {
        let mut args = match &self.test {
            Some(test) if takes(suite.filter, test) => vec![test.as_str(), "--exact"],
            Some(_) => return None,
            None => suite.filter.to_vec(),
        };
        args.extend(suite.output.args());
        Some(args)
    }

    /// What it chose, as the run says it.
    pub(super) fn said(&self) -> String {
        let suites = if self.asked.is_empty() {
            "every suite".to_owned()
        } else {
            self.asked.join(", ")
        };
        match &self.test {
            Some(test) => format!("suites: {suites}, its test {test} alone"),
            None => format!("suites: {suites}"),
        }
    }

    /// Why it runs nothing under the compositors `asked`: every row of
    /// the chosen suite leaves the asked test out by its filter, or the
    /// rows that run it run under other compositors.
    pub(super) fn nothing_runs(&self, asked: &[Compositor]) -> SelectionError {
        let running: Vec<&Suite> = self
            .rows
            .iter()
            .map(|&row| &SUITES[row])
            .filter(|suite| self.args(suite).is_some())
            .collect();
        let asked = names(asked.iter().copied());
        let what = match &self.test {
            Some(test) if running.is_empty() => {
                return SelectionError::LeftOutByFilter {
                    suite: self.asked.join(", "),
                    test: test.clone(),
                    filters: self
                        .rows
                        .iter()
                        .map(|&row| SUITES[row].filter.join(" "))
                        .collect(),
                };
            }
            Some(test) => format!("{test} of {}", self.asked.join(", ")),
            None if self.asked.is_empty() => "every suite".to_owned(),
            None => self.asked.join(", "),
        };
        let theirs = <Compositor as clap::ValueEnum>::value_variants()
            .iter()
            .copied()
            .filter(|compositor| {
                running
                    .iter()
                    .any(|suite| suite.compositors.contains(compositor))
            });
        SelectionError::OtherCompositors {
            what,
            theirs: names(theirs),
            asked,
        }
    }

    /// Fails unless the asked test is one of `listed`, the chosen suite's
    /// compositor tests as its binary lists them.
    pub(super) fn check_test(&self, listed: &[String]) -> Result<(), SelectionError> {
        match &self.test {
            Some(test) if !listed.contains(test) => Err(SelectionError::UnknownTest {
                suite: self.asked.join(", "),
                test: test.clone(),
                tests: listed.to_vec(),
            }),
            _ => Ok(()),
        }
    }
}

/// The suites' names in `table`, each once, in order.
fn suite_names(table: &[Suite]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    table
        .iter()
        .map(|suite| suite.target.name())
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

/// `compositors`' names, joined by commas.
fn names(compositors: impl Iterator<Item = Compositor>) -> String {
    compositors
        .map(Compositor::name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a row run with libtest arguments `filter` runs `test`: as
/// libtest decides, a test runs when it contains one of the filter's
/// words (or there are none) and none of its `--skip` words.
fn takes(filter: &[&str], test: &str) -> bool {
    let mut words: Vec<&str> = Vec::new();
    let mut skips: Vec<&str> = Vec::new();
    let mut args = filter.iter().copied();
    while let Some(arg) = args.next() {
        if arg == "--skip" {
            skips.extend(args.next());
        } else {
            words.push(arg);
        }
    }
    (words.is_empty() || words.iter().any(|word| test.contains(word)))
        && !skips.iter().any(|skip| test.contains(skip))
}

/// The tests `binary --list --ignored --format terse` printed in `output`.
pub(super) fn listed(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::suites::Output;
    use super::*;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn every_suite_without_a_name() {
        let all = Selection::new(&[], None).unwrap();
        assert_eq!(all.rows(), (0..SUITES.len()).collect::<Vec<_>>());
        assert_eq!(all.said(), "suites: every suite");
        assert_eq!(all.args(&SUITES[0]).unwrap(), SUITES[0].filter);
    }

    #[test]
    fn a_name_chooses_every_row_of_that_name() {
        let chosen = Selection::new(&names(&["deskrinch-workspaces compositor"]), None).unwrap();
        assert_eq!(chosen.rows().len(), 3, "its rows split by compositor");
        for &row in chosen.rows() {
            assert_eq!(SUITES[row].target.name(), "deskrinch-workspaces compositor");
        }
        let desktop = Selection::new(&names(&["xtask desktop"]), None).unwrap();
        assert_eq!(desktop.rows().len(), 1);
        assert_eq!(desktop.said(), "suites: xtask desktop");
        let system = "deskrinch-wayland-client compositor on libwayland";
        assert_eq!(
            Selection::new(&names(&[system]), None)
                .unwrap()
                .rows()
                .len(),
            1
        );
    }

    #[test]
    fn an_unknown_name_lists_the_names() {
        let error = Selection::new(&names(&["deskrinch-panel"]), None).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("\"deskrinch-panel\""), "{message}");
        assert!(
            message.contains("\n  deskrinch-panel compositor\n"),
            "{message}"
        );
        assert!(message.contains("\n  xtask desktop"), "{message}");
        let SelectionError::UnknownSuite { names, .. } = error else {
            panic!("{error:?}");
        };
        let unique: BTreeSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "each name once");
    }

    #[test]
    fn a_test_runs_exactly_in_the_rows_that_let_it() {
        let wm = names(&["deskrinch-window-management compositor"]);
        let ending = Selection::new(&wm, Some("session_end::ends_the_session")).unwrap();
        let running = |selection: &Selection| -> Vec<usize> {
            selection
                .rows()
                .iter()
                .copied()
                .filter(|&row| selection.args(&SUITES[row]).is_some())
                .collect()
        };
        assert_eq!(ending.rows().len(), 2, "both rows are built");
        let only = running(&ending);
        assert_eq!(only.len(), 1, "the last row alone runs it");
        let row = &SUITES[only[0]];
        assert_eq!(row.filter, ["session_end::"]);
        assert_eq!(
            ending.args(row).unwrap(),
            ["session_end::ends_the_session", "--exact"]
        );
        assert_eq!(
            ending.said(),
            "suites: deskrinch-window-management compositor, its test \
             session_end::ends_the_session alone"
        );
        let other = Selection::new(&wm, Some("windows::are_listed")).unwrap();
        let only = running(&other);
        assert_eq!(only.len(), 1, "the first row alone runs it");
        assert!(SUITES[only[0]].filter.contains(&"--skip"));
    }

    #[test]
    fn a_test_needs_one_suite() {
        for asked in [
            names(&[]),
            names(&["xtask desktop", "deskrinch-panel compositor"]),
        ] {
            assert!(matches!(
                Selection::new(&asked, Some("a")),
                Err(SelectionError::TestNeedsOneSuite)
            ));
        }
    }

    #[test]
    fn filters_are_read_as_libtest_reads_them() {
        assert!(takes(&[], "a::b"));
        assert!(takes(&["a::"], "a::b"));
        assert!(!takes(&["a::"], "c::b"));
        assert!(!takes(&["--skip", "a::"], "a::b"));
        assert!(takes(&["--skip", "a::", "--skip", "c::"], "d::b"));
        assert!(!takes(&["x::", "--skip", "x::y"], "x::y_z"));
    }

    #[test]
    fn the_listed_tests_are_checked() {
        let output = "cover::covers: test\nlayers::above: test\n\n2 tests, 0 benchmarks\n";
        let tests = listed(output);
        assert_eq!(tests, ["cover::covers", "layers::above"]);
        let one = names(&["deskrinch-panel compositor"]);
        let known = Selection::new(&one, Some("layers::above")).unwrap();
        assert!(known.check_test(&tests).is_ok());
        let unknown = Selection::new(&one, Some("layers::below")).unwrap();
        let message = unknown.check_test(&tests).unwrap_err().to_string();
        assert!(message.contains("\"layers::below\""), "{message}");
        assert!(
            message.contains("\n  cover::covers\n  layers::above"),
            "{message}"
        );
        assert!(Selection::new(&[], None).unwrap().check_test(&[]).is_ok());
    }

    /// The row of the suite named `name`, which has one row.
    fn only_row(name: &str) -> &'static Suite {
        let selection = Selection::new(&names(&[name]), None).unwrap();
        let [row] = selection.rows() else {
            panic!("{name} has {} rows", selection.rows().len());
        };
        &SUITES[*row]
    }

    #[test]
    fn a_row_that_shows_shows_a_named_test_too() {
        let kwin = "deskrinch-test-kwin compositor";
        let shown = only_row(kwin);
        assert_eq!(shown.output, Output::Shown);
        let test = "support::kwin_says_what_it_composites_with_and_which_effects_it_runs";
        let named = Selection::new(&names(&[kwin]), Some(test)).unwrap();
        assert_eq!(
            named.args(shown).unwrap(),
            [test, "--exact", "--show-output"]
        );
        let whole = Selection::new(&names(&[kwin]), None).unwrap();
        assert_eq!(whole.args(shown).unwrap(), ["--show-output"]);
        let captured = only_row("xtask desktop");
        assert_eq!(captured.output, Output::Captured);
        let named = Selection::new(&names(&["xtask desktop"]), Some("t")).unwrap();
        assert_eq!(named.args(captured).unwrap(), ["t", "--exact"]);
    }

    #[test]
    fn a_test_every_row_leaves_out_names_their_filters() {
        let shared = "deskrinch-workspaces compositor on libwayland";
        let error = Selection::new(&names(&[shared]), Some("desktops::are_four"))
            .unwrap()
            .nothing_runs(&[Compositor::Kwin]);
        assert!(
            matches!(error, SelectionError::LeftOutByFilter { .. }),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!(
                "{shared}'s filter leaves desktops::are_four out; its rows' filters are:\n  {}",
                only_row(shared).filter.join(" ")
            )
        );
    }

    #[test]
    fn a_selection_under_other_compositors_names_theirs() {
        let kwin = names(&["deskrinch-test-kwin compositor"]);
        let named = Selection::new(&kwin, Some("t")).unwrap();
        assert_eq!(
            named.nothing_runs(&[Compositor::Sway]).to_string(),
            "t of deskrinch-test-kwin compositor runs under kwin, none of sway"
        );
        let whole = Selection::new(&kwin, None).unwrap();
        assert_eq!(
            whole
                .nothing_runs(&[Compositor::Sway, Compositor::Labwc])
                .to_string(),
            "deskrinch-test-kwin compositor runs under kwin, none of sway, labwc"
        );
    }
}
