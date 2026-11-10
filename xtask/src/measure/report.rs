//! The numbers of a measurement and how they are printed: each run's
//! metrics, and across runs their median and range.
//!
//! Metrics are integers in a fixed unit, so the arithmetic is exact and
//! tested; only printing divides.

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
}

/// Everything one run measured, in the order it is reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Run {
    /// The metrics.
    pub(crate) metrics: Vec<Metric>,
    /// Facts that are not numbers: the GPU adapter, the load average.
    pub(crate) notes: Vec<(String, String)>,
}

impl Run {
    /// Adds a metric.
    pub(crate) fn push(&mut self, name: impl Into<String>, unit: Unit, value: u64) {
        self.metrics.push(Metric {
            name: name.into(),
            unit,
            value,
        });
    }

    /// Adds a note.
    pub(crate) fn note(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.notes.push((name.into(), value.into()));
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
    let mut values: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for run in runs {
        for metric in &run.metrics {
            if !values.contains_key(metric.name.as_str()) {
                names.push((&metric.name, metric.unit));
            }
            values.entry(&metric.name).or_default().push(metric.value);
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, "{title}: {} run(s)", runs.len());
    for wall in [false, true] {
        if wall {
            let _ = writeln!(
                out,
                "\nWall-clock time: NOT comparable between runs or machines; it moves with \
                 the load average noted under each run."
            );
        }
        let width = names.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        let _ = writeln!(
            out,
            "\n  {:width$}  {:>14}  {:>31}  every run",
            "metric", "median", "min .. max"
        );
        for &(name, unit) in names.iter().filter(|(_, u)| (*u == Unit::WallNs) == wall) {
            let all = &values[name];
            let Some((median, min, max)) = spread(all) else {
                continue;
            };
            let each: Vec<String> = all.iter().map(|&v| format(v, unit)).collect();
            let _ = writeln!(
                out,
                "  {name:width$}  {:>14}  {:>14} .. {:>14}  {}",
                format(median, unit),
                format(min, unit),
                format(max, unit),
                each.join(", ")
            );
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
}
