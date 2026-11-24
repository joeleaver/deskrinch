//! The rule that every xtask test file whose tests are ignored for needing a
//! compositor is run somewhere, kept by a check rather than by memory: by a
//! suite of `SUITES` (`cargo xtask compositor-tests`), or as one of the
//! session tests (`cargo xtask session-tests`, [`crate::session_tests`]).
//! A test file left out of both runs only when someone remembers to.
//!
//! A test file is `xtask/tests/NAME.rs`, or `xtask/tests/NAME/main.rs` with
//! the files beside it. It needs a compositor when a test in it is ignored
//! with a reason that names one, however it is worded (`#[ignore = "needs a
//! compositor: …"]`, `"needs a nested compositor: …"`, `"needs Valgrind and
//! a compositor: …"`).

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::super::suites::SUITES;
    use crate::session_tests::SESSION_TESTS;

    /// The workspace's root, above xtask's manifest.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask is in the workspace")
            .to_owned()
    }

    /// Each test file of xtask's, by its test's name, with its sources.
    fn test_files() -> Vec<(String, Vec<PathBuf>)> {
        let tests = root().join("xtask/tests");
        let mut files = Vec::new();
        for entry in fs::read_dir(&tests).unwrap() {
            let path = entry.unwrap().path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if path.extension().is_some_and(|extension| extension == "rs") {
                files.push((stem.to_owned(), vec![path.clone()]));
            } else if path.join("main.rs").is_file() {
                let sources = fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|source| source.extension().is_some_and(|ext| ext == "rs"))
                    .collect();
                files.push((stem.to_owned(), sources));
            }
        }
        files.sort();
        files
    }

    /// Whether `source` has a test ignored for needing a compositor.
    fn needs_a_compositor(source: &str) -> bool {
        source.lines().any(|line| {
            line.trim()
                .strip_prefix("#[ignore = \"")
                .and_then(|rest| rest.split_once('"'))
                .is_some_and(|(reason, _)| reason.contains("compositor"))
        })
    }

    #[test]
    fn every_test_file_that_needs_a_compositor_is_run_by_a_suite_or_as_a_session_test() {
        let mut run: BTreeSet<String> = SESSION_TESTS
            .iter()
            .map(|test| test.name.to_owned())
            .collect();
        run.extend(
            SUITES
                .iter()
                .filter(|suite| suite.target.package == "xtask")
                .map(|suite| suite.target.test.to_owned()),
        );
        let orphans: Vec<String> = test_files()
            .into_iter()
            .filter(|(_, sources)| {
                sources
                    .iter()
                    .any(|source| needs_a_compositor(&fs::read_to_string(source).unwrap()))
            })
            .map(|(name, _)| name)
            .filter(|name| !run.contains(name))
            .collect();
        assert!(
            orphans.is_empty(),
            "xtask's test files {orphans:?} have tests ignored for needing a compositor, and \
             nothing runs them: add each to a suite in xtask/src/compositor_tests/suites/ (a \
             row cargo xtask compositor-tests runs, named in its `SUITES`), or to `SESSION_TESTS` \
             in xtask/src/session_tests.rs"
        );
    }

    #[test]
    fn the_rule_reads_the_reasons_as_they_are_written() {
        assert!(needs_a_compositor(
            "    #[ignore = \"needs a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR\"]"
        ));
        assert!(needs_a_compositor(
            "#[ignore = \"needs Valgrind and a compositor: set DESKRINCH_SESSION_TEST_COMPOSITOR\"]"
        ));
        assert!(needs_a_compositor(
            "#[ignore = \"needs a nested compositor: cargo xtask compositor-tests\"]"
        ));
        assert!(!needs_a_compositor(
            "#[ignore = \"the stand-in measured by this file's tests\"]"
        ));
        assert!(!needs_a_compositor("//! None of it needs a compositor."));
    }
}
