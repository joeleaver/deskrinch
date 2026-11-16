//! A step that ran otherwise in one run than in the others: noted in that
//! run, where the runs are first seen together.
//!
//! Every step (a key of `--type`, a pointer move) has two rows: its frames
//! presented, and how many of them ran reactive work. A reactive frame in a
//! step is no anomaly in itself: typing changes a query, and a pointer over
//! a list moves its selection. What is one is a run whose step differs from
//! the same step in the other runs: its figure may then include work that is
//! not the step's, such as a clock's minute. So a run whose pair of rows for
//! a step matches no other run's is noted, with both pairs. Steps are paired
//! across the runs by their rows' exact labels, prefix included (`show 2:
//! key 1 of 3`). With one run nothing is noted: the rows are then the
//! reader's to compare.

use super::Run;

/// The end of a step's frames row.
const PRESENTED: &str = ": frames presented";
/// The end of a step's reactive frames row.
const REACTIVE: &str = ": of them with reactive work";

/// Notes, in each of `runs`, each step whose pair of rows (frames presented,
/// of them reactive) matches no other run's for that step.
pub(crate) fn note_odd_steps(runs: &mut [Run]) {
    if runs.len() < 2 {
        return;
    }
    for step in steps(runs) {
        let pairs: Vec<Option<(u64, u64)>> = runs.iter().map(|run| pair(run, &step)).collect();
        for index in 0..runs.len() {
            let Some(this) = pairs[index] else {
                continue;
            };
            let others: Vec<(u64, u64)> = pairs
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .filter_map(|(_, pair)| *pair)
                .collect();
            if others.is_empty() || others.contains(&this) {
                continue;
            }
            runs[index].note(step.clone(), note(this, &others));
        }
    }
}

/// The steps the runs report, each once, in the order first seen.
fn steps(runs: &[Run]) -> Vec<String> {
    let mut steps: Vec<String> = Vec::new();
    for metric in runs.iter().flat_map(|run| &run.metrics) {
        if let Some(step) = metric.name.strip_suffix(PRESENTED)
            && !steps.iter().any(|seen| seen == step)
        {
            steps.push(step.to_owned());
        }
    }
    steps
}

/// `run`'s pair of rows for `step`, if it has both.
fn pair(run: &Run, step: &str) -> Option<(u64, u64)> {
    let value = |end: &str| {
        let name = format!("{step}{end}");
        run.metrics
            .iter()
            .find(|metric| metric.name == name)
            .map(|metric| metric.value)
    };
    Some((value(PRESENTED)?, value(REACTIVE)?))
}

/// The note for a run whose pair `this` is none of the other runs' `others`.
fn note((presented, reactive): (u64, u64), others: &[(u64, u64)]) -> String {
    let mut distinct: Vec<(u64, u64)> = Vec::new();
    for other in others {
        if !distinct.contains(other) {
            distinct.push(*other);
        }
    }
    let against = distinct
        .iter()
        .map(|(p, r)| format!("{p} and {r}"))
        .collect::<Vec<_>>()
        .join("; ");
    let runs = if others.len() == 1 {
        "the other run"
    } else {
        "the other runs"
    };
    format!(
        "{presented} presented, {reactive} reactive, against {against} in {runs}: its figure \
         may include work that is not the step's"
    )
}

#[cfg(test)]
mod tests {
    use super::super::Unit;
    use super::*;

    const STEP: &str = "pointer step 1 of 2 (to 1:30,700)";

    /// A run whose step `STEP` presented `presented` frames, `reactive` of
    /// them reactive.
    fn run((presented, reactive): (u64, u64)) -> Run {
        let mut run = Run::default();
        run.push(
            format!("{STEP}: instructions from the move to settled (Ir)"),
            Unit::Count,
            9,
        );
        run.push(format!("{STEP}{PRESENTED}"), Unit::Count, presented);
        run.push(format!("{STEP}{REACTIVE}"), Unit::Count, reactive);
        run
    }

    fn notes(pairs: &[(u64, u64)]) -> Vec<Vec<String>> {
        let mut runs: Vec<Run> = pairs.iter().copied().map(run).collect();
        note_odd_steps(&mut runs);
        runs.iter()
            .map(|run| {
                run.notes
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}"))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn steps_alike_in_every_run_are_not_noted_even_when_reactive() {
        assert_eq!(notes(&[(1, 0), (1, 0), (1, 0)]), [[""; 0]; 3]);
        assert_eq!(notes(&[(1, 1), (1, 1), (1, 1)]), [[""; 0]; 3]);
    }

    #[test]
    fn the_run_that_differs_from_the_others_is_noted_with_both_pairs() {
        let noted = notes(&[(2, 1), (1, 0), (1, 0)]);
        assert_eq!(
            noted[0],
            [format!(
                "{STEP}: 2 presented, 1 reactive, against 1 and 0 in the other runs: its \
                 figure may include work that is not the step's"
            )]
        );
        assert!(noted[1].is_empty() && noted[2].is_empty(), "{noted:?}");
    }

    #[test]
    fn when_every_run_differs_each_is_noted() {
        let noted = notes(&[(1, 0), (2, 1), (3, 1)]);
        assert!(noted.iter().all(|run| run.len() == 1), "{noted:?}");
        assert!(
            noted[1][0].contains("against 1 and 0; 3 and 1 in the other runs"),
            "{noted:?}"
        );
        let two = notes(&[(1, 0), (2, 1)]);
        assert!(two[0][0].contains("1 presented, 0 reactive, against 2 and 1 in the other run:"));
        assert!(two[1][0].contains("2 presented, 1 reactive, against 1 and 0 in the other run:"));
    }

    #[test]
    fn one_run_is_never_noted() {
        assert_eq!(notes(&[(2, 1)]), [[""; 0]; 1]);
    }

    #[test]
    fn steps_are_paired_by_their_whole_label() {
        let mut first = Run::default();
        first.push("show 1: key 1 of 3: frames presented", Unit::Count, 1);
        first.push(
            "show 1: key 1 of 3: of them with reactive work",
            Unit::Count,
            1,
        );
        first.push("show 2: key 1 of 3: frames presented", Unit::Count, 1);
        first.push(
            "show 2: key 1 of 3: of them with reactive work",
            Unit::Count,
            1,
        );
        let mut second = Run::default();
        second.push("show 2: key 1 of 3: frames presented", Unit::Count, 1);
        second.push(
            "show 2: key 1 of 3: of them with reactive work",
            Unit::Count,
            1,
        );
        second.push("show 1: key 1 of 3: frames presented", Unit::Count, 2);
        second.push(
            "show 1: key 1 of 3: of them with reactive work",
            Unit::Count,
            1,
        );
        let mut runs = [first, second];
        note_odd_steps(&mut runs);
        assert_eq!(runs[0].notes.len(), 1, "{:?}", runs[0].notes);
        assert_eq!(runs[0].notes[0].0, "show 1: key 1 of 3");
        assert_eq!(runs[1].notes.len(), 1, "{:?}", runs[1].notes);
        assert_eq!(runs[1].notes[0].0, "show 1: key 1 of 3");
    }
}
