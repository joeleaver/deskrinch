//! A bar on each output, without a UI: the bars there are ([`Bars`]), and
//! which of them show something that is on a set of named outputs
//! ([`outputs_on_bar`]): a window the compositor has placed, for the
//! taskbar; a group of workspaces, for the switcher. The callers decide what
//! their things' outputs are; this crate only matches them with the bars.
//!
//! # The rule
//!
//! Something on the outputs named `outputs` is on the bar of the output
//! named `bar`:
//!
//! - when `outputs` holds `bar`: it is on the bars of its outputs, and on
//!   two outputs it is on both bars;
//! - when none of `outputs` is a bar's output, it is on every bar. Its
//!   outputs may have gone, their names may match no bar's, or it may name
//!   none at all: where it is cannot be told, so it is shown, on more bars
//!   than it belongs to at worst and never on none;
//! - when the bar's own output has no name (or an empty one), the bar shows
//!   it: nothing can be matched with that bar, and a bar that showed
//!   nothing could hide everything.
//!
//! With one bar, everything is on it, by those cases alone: a bar with no
//! name shows everything, and a named one's output either is among
//! `outputs` or none of them is a bar's. So a compositor that names
//! nothing still gives a working panel on one output.
//!
//! So whatever is matched is on at least one bar, always: the property the
//! tests check for every set of outputs and every set of bars.
//!
//! Outputs are matched by name, the `wl_output` name the compositor gives
//! the things' outputs and the panel's alike. Two outputs sharing a name are
//! harmless: what is on that name is shown on both bars.

use std::collections::BTreeSet;

/// The bars there are: how many, and the names of their outputs (a bar on
/// an output with no name counted, not named). The rule reads the names
/// alone; the count is for a caller whose rule differs with one bar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bars {
    /// The names of the bars' outputs.
    pub named: BTreeSet<String>,
    /// How many bars there are.
    pub count: usize,
}

/// Whether something on the outputs named `outputs` is on the bar of the
/// output named `bar` (`None` for an output with no name), among `bars`:
/// the crate's rule.
#[must_use]
pub fn outputs_on_bar(outputs: &BTreeSet<String>, bar: Option<&str>, bars: &Bars) -> bool {
    let Some(bar) = bar.filter(|name| !name.is_empty()) else {
        return true;
    };
    outputs.contains(bar) || outputs.is_disjoint(&bars.named)
}

#[cfg(test)]
mod tests;
