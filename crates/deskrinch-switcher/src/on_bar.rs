//! Which bar's switcher shows which run, when the panel has a bar on each
//! output: a run is on the bars of its group's outputs, by
//! [`deskrinch_bars::outputs_on_bar`], whose documentation has the rule and
//! why each case is shown where it is. A run whose group names no output,
//! and the run of the workspaces in no group the compositor has reported,
//! name none ([`Run::outputs`]), and so are on every bar. So every run is
//! on at least one bar, and with one bar every run is on it.
//!
//! # What the compositors under test give
//!
//! - **KWin**: its virtual desktops are one group, on every output.
//! - **labwc**: ext-workspace, one group.
//! - **sway**: no workspace protocol, and no switcher.
//!
//! None of them has more than one group, so with any of them every bar's
//! switcher shows the same one run, as before there was a bar per output.
//! The path where groups differ by output is proved by this crate's tests
//! and by the panel's test of two bars with two groups, not by a
//! compositor.

use deskrinch_bars::{Bars, outputs_on_bar};

use crate::Run;

/// The runs of `runs` that the switcher on the bar of the output named
/// `bar` (`None` for an output with no name) shows, in their order, among
/// `bars`.
#[must_use]
pub fn runs_on<Id: Clone>(runs: &[Run<Id>], bar: Option<&str>, bars: &Bars) -> Vec<Run<Id>> {
    runs.iter()
        .filter(|run| outputs_on_bar(&run.outputs, bar, bars))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "on_bar_tests.rs"]
mod tests;
