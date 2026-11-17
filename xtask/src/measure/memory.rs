//! A run's memory readings: its status sizes, Pss, the resident set by what
//! is mapped ([`smaps`]), and GPU memory, which is not in the resident set
//! ([`gpu`]).
//!
//! Every run reads it once settled, before the idle window. With
//! `--memory-after-steps` a timed run reads it again once the steps are
//! done and the program has settled after the last, its rows named apart
//! and printed after the steps': what the steps left behind (a device made
//! for a first popup and kept after the last one closed, say).

use std::fs;

use super::probe::sample;
use super::report::{Run, Unit};
use super::{gpu, procfs, smaps};
use crate::error::Error;

/// Which of a run's memory readings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reading {
    /// Settled after start-up (and any setup commands), before the idle
    /// window: every run's.
    Settled,
    /// Settled after the last step: `--memory-after-steps`.
    AfterSteps,
}

impl Reading {
    /// The row of a `/proc/PID/status` field (`VmRSS`), or of `Pss`.
    fn size(self, field: &str) -> String {
        match self {
            Self::Settled => format!("{field} (settled)"),
            Self::AfterSteps => format!("{field} (after the steps, settled)"),
        }
    }

    /// The row of what one group of mappings holds: `kind` is `Rss` or
    /// `Pss`.
    fn group(self, kind: &str, group: &str) -> String {
        match self {
            Self::Settled => format!("  {kind}: {group}"),
            Self::AfterSteps => format!("  {kind} after the steps: {group}"),
        }
    }

    /// The rows of one GPU memory region: its total, and how much of it is
    /// resident.
    fn gpu(self, region: &str) -> (String, String) {
        let region = match self {
            Self::Settled => region.to_owned(),
            Self::AfterSteps => format!("{region} after the steps"),
        };
        (
            format!("GPU memory {region}, total (not in Rss)"),
            format!("GPU memory {region}, resident"),
        )
    }
}

/// Records process `pid`'s memory now, its rows named for `reading`.
pub(crate) fn memory(
    run: &mut Run,
    pid: u32,
    program: &str,
    reading: Reading,
) -> Result<(), Error> {
    let settled = sample(pid, program)?;
    for field in procfs::STATUS_FIELDS {
        let value = settled.status.get(field).copied().unwrap_or(0);
        let unit = if field == "Threads" {
            Unit::Count
        } else {
            Unit::Kib
        };
        run.push(reading.size(field), unit, value);
    }
    let dir = format!("/proc/{pid}");
    let rollup = fs::read_to_string(format!("{dir}/smaps_rollup")).unwrap_or_default();
    run.push(
        reading.size("Pss"),
        Unit::Kib,
        procfs::parse_status(&rollup)
            .get("Pss")
            .copied()
            .unwrap_or(0),
    );
    let exe = fs::read_link(format!("{dir}/exe")).unwrap_or_default();
    let maps = fs::read_to_string(format!("{dir}/smaps")).unwrap_or_default();
    let mut groups: Vec<_> = smaps::parse(&maps, &exe.to_string_lossy())
        .into_iter()
        .collect();
    groups.sort_by_key(|(_, usage)| std::cmp::Reverse(usage.rss));
    for (group, usage) in groups.into_iter().filter(|(_, usage)| usage.rss > 0) {
        run.push(reading.group("Rss", group), Unit::Kib, usage.rss);
        run.push(reading.group("Pss", group), Unit::Kib, usage.pss);
    }
    for (region, (total, resident)) in gpu::sum(&gpu::read(pid)) {
        let (total_row, resident_row) = reading.gpu(&region);
        run.push(total_row, Unit::Kib, total);
        run.push(resident_row, Unit::Kib, resident);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settled_reading_keeps_its_names_and_the_second_is_named_apart() {
        let settled = Reading::Settled;
        assert_eq!(settled.size("VmRSS"), "VmRSS (settled)");
        assert_eq!(settled.group("Rss", "the heap"), "  Rss: the heap");
        assert_eq!(
            settled.gpu("GTT"),
            (
                "GPU memory GTT, total (not in Rss)".to_owned(),
                "GPU memory GTT, resident".to_owned()
            )
        );
        let after = Reading::AfterSteps;
        assert_eq!(after.size("VmRSS"), "VmRSS (after the steps, settled)");
        assert_eq!(
            after.group("Pss", "the heap"),
            "  Pss after the steps: the heap"
        );
        assert_eq!(
            after.gpu("VRAM").0,
            "GPU memory VRAM after the steps, total (not in Rss)"
        );
    }
}
