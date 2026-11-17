//! Which bar's taskbar shows which window, when the panel has a bar on each
//! output: a window is shown on the bars of the outputs it is on, and never
//! on none.
//!
//! # The rule
//!
//! - A window's **known outputs** are the last outputs it reported that
//!   were not none ([`known_outputs`]). A window that has never reported
//!   any knows none.
//! - A window is on a bar when its known outputs include that bar's output
//!   ([`on_bar`]). A window on two outputs is on both bars.
//! - If none of its known outputs is the output of a bar there is (it
//!   knows none, the ones it knows have gone, or their names match no
//!   bar's), it is on every bar.
//! - A bar whose own output has no name (or an empty one) shows every
//!   window.
//!
//! So every window the taskbar shows is on at least one bar, always: the
//! property the tests check for every window and every set of bars.
//!
//! Outputs are matched by name, the `wl_output` name the compositor gives
//! both the window's outputs and the panel's. Two outputs sharing a name,
//! or an output with an empty name, are harmless: the window is shown,
//! on more bars than it is on at worst, never on fewer.
//!
//! # Why the known outputs outlive a report of none
//!
//! A window reports no output only for an instant, while the compositor
//! moves it back to an output plugged in again; reported as it is, that
//! instant would put its button on every bar and take it off again, a
//! layout and a frame on each. Recorded with a test window under each
//! compositor the tests run (two outputs; the window on the first, which
//! is unplugged and plugged in again), the window-management events said:
//!
//! - **KWin 6.6.6**: at the unplug, both outputs, then the other one alone;
//!   at the replug, none, the other one, none, then its own, all within one
//!   millisecond.
//! - **labwc 0.9.3**: at the unplug, both outputs, then the other one; at
//!   the replug, none, then its own.
//! - **sway 1.11**: at the unplug, the other output directly; at the replug,
//!   nothing: sway leaves the window on the other output.
//!
//! A minimized window, and one on a workspace not shown, kept reporting its
//! output under all three (sway has no minimize). Kept through the reports
//! of none, the known outputs move the window from its old output's bar to
//! its new one's with no instant on every bar. While its only known output
//! is unplugged and the compositor has not yet said where the window went,
//! the third line of the rule shows it on every bar that remains.

use std::collections::BTreeSet;

use crate::Button;

/// A window's known outputs once it reports `reported`, having known
/// `known`: what it reported, unless that is none, when what it knew is
/// kept.
#[must_use]
pub fn known_outputs(known: &BTreeSet<String>, reported: BTreeSet<String>) -> BTreeSet<String> {
    if reported.is_empty() {
        known.clone()
    } else {
        reported
    }
}

/// Whether a window whose known outputs are `known` is on the bar of the
/// output named `bar` (`None` for an output with no name), when the bars
/// there are are on the outputs named `bars`.
#[must_use]
pub fn on_bar(known: &BTreeSet<String>, bar: Option<&str>, bars: &BTreeSet<String>) -> bool {
    let Some(bar) = bar.filter(|name| !name.is_empty()) else {
        return true;
    };
    known.contains(bar) || known.is_disjoint(bars)
}

/// The buttons of `buttons` that the bar of the output named `bar` shows,
/// in their order, when the bars there are are on the outputs named `bars`
/// ([`on_bar`]).
#[must_use]
pub fn shown_on<Id: Clone>(
    buttons: &[Button<Id>],
    bar: Option<&str>,
    bars: &BTreeSet<String>,
) -> Vec<Button<Id>> {
    buttons
        .iter()
        .filter(|button| on_bar(&button.outputs, bar, bars))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
