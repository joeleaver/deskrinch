//! The machine a measurement ran on, for its report.

use std::fs;

/// The CPU model, its logical CPUs and the kernel, in one line.
pub(crate) fn describe() -> String {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    format!(
        "{}, {} logical CPUs, Linux {}",
        cpu_model(&cpuinfo).unwrap_or("unknown CPU"),
        logical_cpus(&cpuinfo),
        kernel.trim()
    )
}

/// The first `model name` in `/proc/cpuinfo`.
fn cpu_model(cpuinfo: &str) -> Option<&str> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim())
    })
}

/// How many `processor` entries `/proc/cpuinfo` has.
fn logical_cpus(cpuinfo: &str) -> usize {
    cpuinfo
        .lines()
        .filter(|line| {
            line.split(':')
                .next()
                .is_some_and(|key| key.trim() == "processor")
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_model_and_the_cpu_count() {
        let cpuinfo = "processor\t: 0\nmodel name\t: AMD Ryzen AI 9 HX 370\n\n\
                       processor\t: 1\nmodel name\t: AMD Ryzen AI 9 HX 370\n";
        assert_eq!(cpu_model(cpuinfo), Some("AMD Ryzen AI 9 HX 370"));
        assert_eq!(logical_cpus(cpuinfo), 2);
        assert_eq!(cpu_model(""), None);
    }
}
