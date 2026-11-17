//! The numbers of a measurement and how they are printed: each run's
//! metrics, and across runs their median and range.
//!
//! Metrics are integers in a fixed unit, so the arithmetic is exact and
//! tested; only printing divides.

mod odd_steps;

pub(crate) use odd_steps::note_odd_steps;

use deskrinch_test_component::Presented;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// What a metric's integer counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    /// A plain count: frames, threads, instructions, context switches.
    Count,
    /// Kibibytes, printed as MiB.
    Kib,
    /// Nanoseconds of CPU time, printed as milliseconds.
    CpuNs,
    /// Thousandths, printed with two decimals: a rate per second.
    Milli,
    /// Nanoseconds of wall-clock time, printed as milliseconds, and only
    /// beside the load average: it moves with the machine's load.
    WallNs,
}

/// One named number of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Metric {
    /// What it is, as the report labels it.
    pub(crate) name: String,
    /// Its unit.
    pub(crate) unit: Unit,
    /// Its value in that unit.
    pub(crate) value: u64,
    /// Why it has no value in this run, printed in its place (`no frame`);
    /// `value` is then not a measurement and is not printed.
    pub(crate) instead: Option<&'static str>,
}

/// Everything one run measured, in the order it is reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Run {
    /// The metrics.
    pub(crate) metrics: Vec<Metric>,
    /// Facts that are not numbers: the GPU adapter, the load average.
    pub(crate) notes: Vec<(String, String)>,
    /// Lines printed under a metric's row, once whatever the runs: what
    /// the rows above it mean, where their names could mislead. Each is
    /// the row it goes under, and the line.
    pub(crate) remarks: Vec<(String, String)>,
    /// Which renderer presented, as the program said.
    pub(crate) presented: Option<Presented>,
    /// Whether the program presented a frame or opened a window in the run:
    /// a program with no window (a daemon with nothing to show) did not.
    pub(crate) drew: bool,
    /// Everything the program wrote, oldest first ([`super::output`] keeps
    /// it in a file and checks it).
    pub(crate) output: Vec<String>,
}

impl Run {
    /// Adds a metric.
    pub(crate) fn push(&mut self, name: impl Into<String>, unit: Unit, value: u64) {
        self.metrics.push(Metric {
            name: name.into(),
            unit,
            value,
            instead: None,
        });
    }

    /// Adds a metric that has no value in this run, `why` printed in its
    /// place: a step's wall time to a frame it did not lead to.
    pub(crate) fn push_without(&mut self, name: impl Into<String>, unit: Unit, why: &'static str) {
        self.metrics.push(Metric {
            name: name.into(),
            unit,
            value: 0,
            instead: Some(why),
        });
    }

    /// Adds `line` under the row of the metric named `after`.
    pub(crate) fn remark(&mut self, after: impl Into<String>, line: impl Into<String>) {
        self.remarks.push((after.into(), line.into()));
    }

    /// Adds a note.
    pub(crate) fn note(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.notes.push((name.into(), value.into()));
    }

    /// Sets the note `name` to `value` where it is, or adds it: a fact read
    /// at the end of the run that keeps its place among the notes.
    pub(crate) fn set_note(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.notes.iter_mut().find(|(noted, _)| noted == name) {
            Some((_, noted)) => *noted = value,
            None => self.notes.push((name.to_owned(), value)),
        }
    }
}

/// `count` events over `millis` milliseconds, per second, in thousandths.
pub(crate) fn per_second_milli(count: u64, millis: u64) -> u64 {
    if millis == 0 {
        return 0;
    }
    let value = u128::from(count) * 1_000_000 / u128::from(millis);
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// `amount` accrued over `millis` milliseconds, scaled to one minute.
pub(crate) fn per_minute(amount: u64, millis: u64) -> u64 {
    if millis == 0 {
        return 0;
    }
    let value = u128::from(amount) * 60_000 / u128::from(millis);
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// The median of `values` (the lower middle one of an even count), and
/// the smallest and largest; `None` for no values.
pub(crate) fn spread(values: &[u64]) -> Option<(u64, u64, u64)> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let median = *sorted.get((sorted.len().checked_sub(1)?) / 2)?;
    Some((median, *sorted.first()?, *sorted.last()?))
}

/// `value` in `unit`, as printed.
pub(crate) fn format(value: u64, unit: Unit) -> String {
    match unit {
        Unit::Count => group_thousands(value),
        Unit::Kib => format!("{} MiB", decimal(value * 10 / 1024, 1)),
        Unit::CpuNs | Unit::WallNs => format!("{} ms", decimal(value / 100_000, 1)),
        Unit::Milli => decimal(value / 10, 2),
    }
}

/// `value` with its last `places` digits after a decimal point.
fn decimal(value: u64, places: u32) -> String {
    let scale = 10_u64.pow(places);
    format!(
        "{}.{:0width$}",
        group_thousands(value / scale),
        value % scale,
        width = places as usize
    )
}

/// `1234567` as `1,234,567`.
fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The report of `runs`: per metric its median, range and every run's
/// value, the wall-clock metrics apart and marked, then each run's notes.
pub(crate) fn render(title: &str, runs: &[Run]) -> String {
    let mut names: Vec<(&str, Unit)> = Vec::new();
    // Each run's value, or why it has none.
    let mut values: BTreeMap<&str, Vec<Result<u64, &str>>> = BTreeMap::new();
    for run in runs {
        for metric in &run.metrics {
            if !values.contains_key(metric.name.as_str()) {
                names.push((&metric.name, metric.unit));
            }
            values
                .entry(&metric.name)
                .or_default()
                .push(metric.instead.map_or(Ok(metric.value), Err));
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, "{title}: {} run(s)", runs.len());
    let width = names.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    for wall in [false, true] {
        let section: Vec<(&str, Unit)> = names
            .iter()
            .copied()
            .filter(|(_, unit)| (*unit == Unit::WallNs) == wall)
            .collect();
        if section.is_empty() {
            continue;
        }
        if wall {
            let _ = writeln!(
                out,
                "\nWall-clock time: NOT comparable between runs or machines; it moves with \
                 the load average noted under each run."
            );
        }
        let _ = writeln!(
            out,
            "\n  {:width$}  {:>14}  {:>31}  every run",
            "metric", "median", "min .. max"
        );
        for (name, unit) in section {
            let all = &values[name];
            let measured: Vec<u64> = all.iter().filter_map(|value| value.ok()).collect();
            // The spread of the runs that have a value; where none has, why.
            let (median, min, max) = match (spread(&measured), all.first()) {
                (Some((median, min, max)), _) => {
                    (format(median, unit), format(min, unit), format(max, unit))
                }
                (None, Some(Err(why))) => ((*why).to_owned(), (*why).to_owned(), (*why).to_owned()),
                (None, _) => continue,
            };
            let each: Vec<String> = all
                .iter()
                .map(|value| match value {
                    Ok(value) => format(*value, unit),
                    Err(why) => (*why).to_owned(),
                })
                .collect();
            let missing = if all.len() < runs.len() {
                format!(" (in {} of {} runs)", all.len(), runs.len())
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "  {name:width$}  {median:>14}  {min:>14} .. {max:>14}  {}{missing}",
                each.join(", ")
            );
            let remark = runs
                .iter()
                .flat_map(|run| &run.remarks)
                .find(|(after, _)| after == name);
            if let Some((_, line)) = remark {
                let _ = writeln!(out, "    {line}");
            }
        }
    }
    for (index, run) in runs.iter().enumerate() {
        if run.notes.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\nRun {}:", index + 1);
        for (name, value) in &run.notes {
            let _ = writeln!(out, "  {name}: {value}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_are_exact_integers() {
        // 15 wake-ups in 125.3 s: 0.119 per second.
        assert_eq!(per_second_milli(15, 125_300), 119);
        assert_eq!(per_second_milli(5, 1000), 5000);
        assert_eq!(per_second_milli(1, 0), 0);
        // 12.1 ms of CPU in 125.7 s is 5.77 ms a minute.
        assert_eq!(per_minute(12_100_000, 125_700), 5_775_656);
        assert_eq!(per_minute(1, 0), 0);
    }

    #[test]
    fn spread_takes_the_lower_middle_of_an_even_count() {
        assert_eq!(spread(&[30, 10, 20]), Some((20, 10, 30)));
        assert_eq!(spread(&[40, 10, 30, 20]), Some((20, 10, 40)));
        assert_eq!(spread(&[7]), Some((7, 7, 7)));
        assert_eq!(spread(&[]), None);
    }

    #[test]
    fn formats_each_unit() {
        assert_eq!(format(2_310_257_585, Unit::Count), "2,310,257,585");
        assert_eq!(format(999, Unit::Count), "999");
        assert_eq!(format(109_996, Unit::Kib), "107.4 MiB");
        assert_eq!(format(195_872, Unit::Kib), "191.2 MiB");
        assert_eq!(format(850_000_000, Unit::CpuNs), "850.0 ms");
        assert_eq!(format(1_234_567_890, Unit::WallNs), "1,234.5 ms");
        assert_eq!(format(119, Unit::Milli), "0.11");
        assert_eq!(format(5000, Unit::Milli), "5.00");
    }

    #[test]
    fn the_report_keeps_wall_time_apart() {
        let mut first = Run::default();
        first.push("CPU to first frame", Unit::CpuNs, 850_000_000);
        first.push("wall time to first frame", Unit::WallNs, 2_859_000_000);
        first.note("load average", "106.47");
        let mut second = Run::default();
        second.push("CPU to first frame", Unit::CpuNs, 330_000_000);
        second.push("wall time to first frame", Unit::WallNs, 937_000_000);
        let report = render("panel", &[first, second]);
        let (numbers, wall) = report
            .split_once("Wall-clock time")
            .expect("a wall section");
        assert!(numbers.contains("CPU to first frame"));
        assert!(numbers.contains("330.0 ms ..       850.0 ms"));
        assert!(!numbers.contains("wall time to first frame"));
        assert!(wall.contains("wall time to first frame"));
        assert!(report.contains("Run 1:\n  load average: 106.47"));
        assert!(
            !report.contains("Run 2:"),
            "a run without notes has no section"
        );
    }

    #[test]
    fn a_remark_is_printed_once_under_its_row() {
        let mut first = Run::default();
        first.push("when a came", Unit::CpuNs, 1_000_000);
        first.push("when b came", Unit::CpuNs, 2_000_000);
        first.remark("when b came", "(cumulative)");
        first.push("after", Unit::Count, 1);
        let mut second = first.clone();
        second.metrics[0].value = 3_000_000;
        let report = render("x", &[first, second]);
        let b = report.find("when b came").expect("row b");
        let after = report.find("after").expect("row after");
        let remark = report.find("    (cumulative)\n").expect("the remark");
        assert!(b < remark && remark < after, "{report}");
        assert_eq!(report.matches("(cumulative)").count(), 1, "{report}");
    }

    #[test]
    fn a_metric_without_a_value_prints_why_and_never_a_zero() {
        let mut first = Run::default();
        first.push_without("step 1: wall time", Unit::WallNs, "no frame");
        first.push("step 2: wall time", Unit::WallNs, 4_000_000);
        let mut second = Run::default();
        second.push_without("step 1: wall time", Unit::WallNs, "no frame");
        second.push_without("step 2: wall time", Unit::WallNs, "no frame");
        let report = render("x", &[first, second]);
        let step_1 = report
            .lines()
            .find(|line| line.contains("step 1: wall time"))
            .expect("step 1's row");
        assert!(
            step_1.contains("no frame .. ") && step_1.ends_with("no frame, no frame"),
            "{report}"
        );
        assert!(!step_1.contains("0.0 ms"), "{report}");
        // A run with a value and one without: the spread of the one, and
        // each run as it was.
        assert!(
            report.contains("4.0 ms ..         4.0 ms  4.0 ms, no frame"),
            "{report}"
        );
    }

    #[test]
    fn a_note_set_later_keeps_its_place() {
        let mut run = Run::default();
        run.note("renderer", "not yet");
        run.note("granted", "nothing");
        run.set_note("renderer", "software");
        run.set_note("new", "added");
        assert_eq!(
            run.notes,
            [
                ("renderer".to_owned(), "software".to_owned()),
                ("granted".to_owned(), "nothing".to_owned()),
                ("new".to_owned(), "added".to_owned()),
            ]
        );
    }

    #[test]
    fn a_metric_some_runs_lack_says_so_and_an_empty_section_is_left_out() {
        let mut first = Run::default();
        first.push("instructions", Unit::Count, 10);
        first.push("  thread worker", Unit::CpuNs, 1_000_000);
        let mut second = Run::default();
        second.push("instructions", Unit::Count, 12);
        let report = render("x", &[first, second]);
        assert!(report.contains("1.0 ms (in 1 of 2 runs)"), "{report}");
        assert!(report.contains("10, 12\n"), "{report}");
        assert!(!report.contains("Wall-clock"), "{report}");
    }
}
