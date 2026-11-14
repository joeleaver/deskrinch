//! The measured program's own output: kept, whole, in a file for every run
//! (named in the run's notes), and read for errors. A program that logged
//! at error level said it is broken (a taskbar that could not reach the
//! window list, say), and a measurement of it is not one: the run fails,
//! with the lines, unless `--allow-errors`, which keeps them in the notes.

use std::fs;
use std::path::{Path, PathBuf};

use deskrinch_test_component::Watch;
use deskrinch_test_component::log::is_error;

use super::report::Run;
use crate::error::Error;

/// How many error lines a note or an error message quotes.
const QUOTED: usize = 10;

/// `result`, a mode's run, with everything `watch` saw the program write.
pub(crate) fn with_output(result: Result<Run, Error>, watch: &Watch) -> Result<Run, Error> {
    result.map(|mut run| {
        run.output = watch.lines();
        run
    })
}

/// Where a measurement's runs keep their output: a directory of its own,
/// which outlives the measurement, one file a run.
#[derive(Debug)]
pub(crate) struct Kept {
    dir: PathBuf,
}

impl Kept {
    /// A new directory for a measurement's output.
    pub(crate) fn new() -> Result<Self, Error> {
        let dir = tempfile::Builder::new()
            .prefix("deskrinch-measure-output-")
            .tempdir()
            .map_err(|source| Error::Io {
                what: "create the directory for the program's output".into(),
                source,
            })?
            .keep();
        Ok(Self { dir })
    }

    /// Run `index`'s file.
    pub(crate) fn file(&self, index: u32) -> PathBuf {
        self.dir.join(format!("run-{index}.log"))
    }
}

/// Writes `run`'s output to `file` and names the file in its notes; then,
/// when the program logged errors, fails unless `allow_errors`, when the
/// lines go in the notes instead.
pub(crate) fn settle(
    run: &mut Run,
    file: &Path,
    program: &str,
    allow_errors: bool,
) -> Result<(), Error> {
    let mut text = run.output.join("\n");
    text.push('\n');
    fs::write(file, text).map_err(|source| Error::Io {
        what: format!("write {}", file.display()),
        source,
    })?;
    run.note("the program's output", file.display().to_string());
    let errors = errors(&run.output);
    if errors.is_empty() {
        return Ok(());
    }
    let quoted = quote(&errors);
    if !allow_errors {
        return Err(Error::MeasuredLoggedErrors {
            program: program.to_owned(),
            count: errors.len(),
            lines: quoted,
            log: file.to_owned(),
        });
    }
    run.note(
        format!("errors the program logged ({})", errors.len()),
        quoted,
    );
    Ok(())
}

/// The lines of `output` logged at error level.
fn errors(output: &[String]) -> Vec<&str> {
    output
        .iter()
        .map(String::as_str)
        .filter(|line| is_error(line))
        .collect()
}

/// The first [`QUOTED`] of `errors`, one a line, and how many more.
fn quote(errors: &[&str]) -> String {
    let mut lines: Vec<String> = errors
        .iter()
        .take(QUOTED)
        .map(|&line| line.to_owned())
        .collect();
    if errors.len() > QUOTED {
        lines.push(format!("(and {} more)", errors.len() - QUOTED));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR: &str = "2026-11-13T21:18:32Z ERROR deskrinch_component::report: following the \
                         windows (the taskbar shows none): not granted";

    fn run(lines: &[&str]) -> Run {
        Run {
            output: lines.iter().map(|&line| line.to_owned()).collect(),
            ..Run::default()
        }
    }

    #[test]
    fn a_clean_run_keeps_its_output_and_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run-1.log");
        let mut clean = run(&["[PERF] frame 1: style 0.10ms", "2026 INFO x: fine"]);
        settle(&mut clean, &file, "panel", false).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "[PERF] frame 1: style 0.10ms\n2026 INFO x: fine\n"
        );
        assert_eq!(
            clean.notes,
            [(
                "the program's output".to_owned(),
                file.display().to_string()
            )]
        );
    }

    #[test]
    fn a_run_whose_program_logged_an_error_fails() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run-1.log");
        let mut broken = run(&["[PERF] frame 1: style 0.10ms", ERROR]);
        let error = settle(&mut broken, &file, "panel", false).unwrap_err();
        let Error::MeasuredLoggedErrors {
            count, lines, log, ..
        } = &error
        else {
            panic!("{error:?}");
        };
        assert_eq!(
            (*count, lines.as_str(), log.as_path()),
            (1, ERROR, file.as_path())
        );
        assert!(error.to_string().contains("--allow-errors"));
        assert!(
            fs::read_to_string(&file).unwrap().contains(ERROR),
            "kept all the same"
        );
    }

    #[test]
    fn allowed_errors_go_in_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run-1.log");
        let mut broken = run(&[ERROR, ERROR]);
        settle(&mut broken, &file, "panel", true).unwrap();
        assert_eq!(
            broken.notes[1],
            (
                "errors the program logged (2)".to_owned(),
                format!("{ERROR}\n{ERROR}")
            )
        );
    }

    #[test]
    fn many_errors_are_quoted_in_part() {
        let errors = vec!["ERROR x: y"; QUOTED + 3];
        let quoted = quote(&errors);
        assert_eq!(quoted.lines().count(), QUOTED + 1);
        assert!(quoted.ends_with("(and 3 more)"));
    }
}
