//! What apt had fetched when a run was cut off: the files it had begun
//! (its `Get:` lines) and, once the download was done, apt's own report of
//! it (its `Fetched` line).

use std::time::Duration;

use super::classify::Step;

/// What a run cut off by its limit had fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Progress {
    /// The command.
    pub(crate) step: Step,
    /// How long it had run when it was cut off.
    pub(crate) limit: Duration,
    /// The files it had begun to fetch.
    pub(crate) files: usize,
    /// Their sizes, as apt states them, in kB.
    pub(crate) kilobytes: u64,
    /// The last of them: a package's name, or an index (`resolute
    /// InRelease`).
    pub(crate) last: String,
    /// apt's report of the finished download, when it had finished.
    pub(crate) downloaded: Option<Downloaded>,
}

/// apt's report of a finished download: `Fetched 52.2 MB in 11min 3s
/// (78.7 kB/s)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Downloaded {
    /// The report after `Fetched `, as apt wrote it.
    pub(crate) report: String,
    /// How long it says the download took.
    pub(crate) took: Duration,
}

impl Progress {
    /// Whether the download is what used the run's time: still going when
    /// it was cut off, or finished in more than half of it.
    pub(crate) fn download_used_the_time(&self) -> bool {
        self.downloaded
            .as_ref()
            .is_none_or(|downloaded| downloaded.took * 2 >= self.limit)
    }
}

/// What `output`, a run of `step` cut off after `limit`, shows of what apt
/// fetched; `None` when it began no file.
pub(crate) fn progress(step: Step, limit: Duration, output: &str) -> Option<Progress> {
    let gets: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("Get:"))
        .collect();
    let last = gets.last()?;
    Some(Progress {
        step,
        limit,
        files: gets.len(),
        kilobytes: gets.iter().filter_map(|line| kilobytes(line)).sum(),
        last: fetched_item(last),
        downloaded: output.lines().find_map(downloaded),
    })
}

/// What a `Get:` line fetches: a package's name, where the line names one
/// (`Get:6 <url> resolute-updates/main amd64v3 libc6 amd64 2.43-2ubuntu2.7
/// [2095 kB]`), else what follows the URL (`Get:1 <url> resolute InRelease
/// [126 kB]`, `Get:5 <url> resolute/main amd64 Packages [1,401 kB]`).
pub(crate) fn fetched_item(line: &str) -> String {
    let without_size = line
        .rsplit_once(" [")
        .map_or(line, |(before, _)| before)
        .trim();
    let words: Vec<&str> = without_size.split_whitespace().collect();
    match words.as_slice() {
        // `Get:N`, the URL, the suite, the archive's architecture, the
        // package, its architecture, its version.
        [_, _, _, _, package, _, _] => (*package).to_owned(),
        [_, _, rest @ ..] if !rest.is_empty() => rest.join(" "),
        _ => without_size.to_owned(),
    }
}

/// The size at the end of a `Get:` line (`[1,810 kB]`, `[2.1 MB]`, `[912 B]`),
/// in kB.
pub(crate) fn kilobytes(line: &str) -> Option<u64> {
    let (_, size) = line.trim_end().strip_suffix(']')?.rsplit_once('[')?;
    let (number, unit) = size.split_once(' ')?;
    let number: f64 = number.replace(',', "").parse().ok()?;
    let factor = match unit {
        "B" => 0.001,
        "kB" => 1.0,
        "MB" => 1000.0,
        _ => return None,
    };
    // A file's size in kB, rounded: far inside u64.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a package's size in kB, non-negative and far below u64::MAX"
    )]
    Some((number * factor).round() as u64)
}

/// apt's `Fetched` line, if `line` is it.
fn downloaded(line: &str) -> Option<Downloaded> {
    let report = line.strip_prefix("Fetched ")?;
    let (_, after) = report.split_once(" in ")?;
    let took = duration(after.split(" (").next()?)?;
    Some(Downloaded {
        report: report.trim().to_owned(),
        took,
    })
}

/// A duration as apt writes it: `5s`, `4min 37s`, `1h 2min 3s`.
fn duration(text: &str) -> Option<Duration> {
    let mut seconds = 0;
    for part in text.split_whitespace() {
        let (number, unit) = part.split_at(part.find(|c: char| !c.is_ascii_digit())?);
        let number: u64 = number.parse().ok()?;
        seconds += number
            * match unit {
                "s" => 1,
                "min" => 60,
                "h" => 3600,
                "d" => 86400,
                _ => return None,
            };
    }
    Some(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_in_each_unit() {
        assert_eq!(kilobytes("Get:1 x [912 B]"), Some(1));
        assert_eq!(kilobytes("Get:1 x [1,810 kB]"), Some(1810));
        assert_eq!(kilobytes("Get:1 x [2.1 MB]"), Some(2100));
        assert_eq!(kilobytes("Get:1 x"), None);
    }

    /// An index and a list of `update` (lines of CI's logs, 2026-11-17),
    /// and a package of `install`.
    #[test]
    fn what_a_line_fetches_is_its_package_or_its_index() {
        assert_eq!(
            fetched_item(
                "Get:4 http://azure.archive.ubuntu.com/ubuntu resolute-updates InRelease [137 kB]"
            ),
            "resolute-updates InRelease"
        );
        assert_eq!(
            fetched_item(
                "Get:7 https://packages.microsoft.com/ubuntu/26.04/prod resolute/main arm64 Packages [148 kB]"
            ),
            "resolute/main arm64 Packages"
        );
        assert_eq!(
            fetched_item(
                "Get:6 http://us-east-2.ec2.archive.ubuntu.com/ubuntu resolute-updates/main amd64v3 libc6 amd64 2.43-2ubuntu2.7 [2095 kB]"
            ),
            "libc6"
        );
    }

    #[test]
    fn apts_durations_read() {
        assert_eq!(duration("5s"), Some(Duration::from_secs(5)));
        assert_eq!(duration("4min 37s"), Some(Duration::from_secs(277)));
        assert_eq!(duration("1h 2min 3s"), Some(Duration::from_secs(3723)));
        assert_eq!(duration("soon"), None);
    }

    #[test]
    fn the_fetched_line_is_the_finished_download() {
        assert_eq!(
            downloaded("Fetched 15.4 MB in 4min 37s (55.6 kB/s)"),
            Some(Downloaded {
                report: "15.4 MB in 4min 37s (55.6 kB/s)".into(),
                took: Duration::from_secs(277),
            })
        );
        assert_eq!(downloaded("Get:1 x [1 kB]"), None);
    }
}
