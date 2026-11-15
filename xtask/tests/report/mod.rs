//! Reading `cargo xtask measure`'s report, for the measure tests.

/// The number after `label` in `report` (its first occurrence), its
/// digits' separators removed.
pub(crate) fn number_after(report: &str, label: &str) -> u64 {
    let at = report
        .find(label)
        .unwrap_or_else(|| panic!("no {label:?} in:\n{report}"));
    report[at + label.len()..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("no number after {label:?} in:\n{report}"))
}
