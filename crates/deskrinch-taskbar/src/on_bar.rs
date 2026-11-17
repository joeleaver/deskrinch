//! Which bar's taskbar shows which window, when the panel has a bar on each
//! output: a window is shown on the bars of the outputs it is on.
//!
//! # The rule
//!
//! - A window is **placed** once it has reported outputs that were not
//!   none; its **known outputs** are then the last such outputs it
//!   reported, kept while it reports none ([`Placement`],
//!   [`Placement::after`]). Until then it is **unplaced**, since it was
//!   announced.
//! - With one bar, every window is on it, placed or not.
//! - With several bars ([`on_bar`]):
//!   - a placed window is on the bars of its known outputs (a window on
//!     two outputs is on both), and on every bar when none of its known
//!     outputs is a bar's (they have gone, or their names match no bar's);
//!   - a bar whose own output has no name (or an empty one) shows every
//!     placed window;
//!   - an unplaced window is on no bar for [`GRACE`] from its announcement,
//!     and then, while it is still unplaced, on every bar: where it is
//!     cannot be told, so it is shown. Placed within the grace, it joins its
//!     own bars and was never on another; placed later, it leaves the
//!     others in one step.
//!
//! So with one bar every window is on it, and with several every window
//! that is placed, or whose grace is over, is on at least one: the
//! property the tests check for every window and every set of bars.
//!
//! The time is handed in ([`on_bar`]'s `now`), never read here; the panel
//! wakes once at the earliest grace's end ([`next_deadline`]), and only
//! while some window is unplaced and inside its grace.
//!
//! Outputs are matched by name, the `wl_output` name the compositor gives
//! both the window's outputs and the panel's. Two outputs sharing a name,
//! or an output with an empty name, are harmless: the window is shown,
//! on more bars than it is on at worst, never on fewer.
//!
//! # When a new window first reports an output
//!
//! Recorded at the protocol (a test window opened, two outputs):
//!
//! - **labwc 0.9.3**: its first batch carries the output, for an ordinary
//!   window, one sent to another workspace by a window rule, one iconified
//!   by a rule, and one fullscreen from its first configure.
//! - **sway 1.11**: its first batch (`title`, `state`, `app_id`, `done`)
//!   carries none; `output_enter` and `done` follow 0.003 to 4.2 ms later
//!   for an ordinary window, after its first arrange. A window that opens on
//!   a workspace not shown (a `for_window` move), or fullscreen, gets no
//!   `output_enter` at all while it stays so: sway tells a window's output
//!   as it is shown on it.
//! - **KWin 6.6.6**: announced (`initial_state`) before it is mapped, a
//!   window has no geometry yet, and its first `geometry`, which its outputs
//!   come from, follows 9 to 24 ms later; announced after, its first batch
//!   carries it. A window a script sends to another desktop, minimizes, or
//!   one fullscreen from its first configure is placed all the same.
//!
//! # Why the known outputs outlive a report of none
//!
//! A placed window reports no output only for an instant, while the
//! compositor moves it back to an output plugged in again; reported as it
//! is, that instant would put its button on every bar and take it off
//! again, a layout and a frame on each. Recorded with a test window (two
//! outputs; the window on the first, which is unplugged and plugged in
//! again), the window-management events said:
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
//! output under all three (sway has no minimize). While a window's only
//! known output is unplugged and the compositor has not yet said where it
//! went, the rule shows it on every bar that remains.
//!
//! # Limits
//!
//! Both are sway's (1.11), and nothing here works round them:
//!
//! - A window sway never places (opened on a workspace not shown, or
//!   fullscreen, until it is shown or leaves fullscreen) is on every bar.
//! - A window made fullscreen on another output is still told as on its
//!   old one until it leaves fullscreen, so it stays on the old output's
//!   bar until then.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::Button;

/// How long a new window may stay unplaced before it is shown on every
/// bar. sway tells an ordinary window's output after its first arrange,
/// which it bounds with its transaction timeout: 200 ms by default
/// (`server->txn_timeout_ms`, sway 1.11's `sway/server.c`). KWin's gap
/// between announcing a window and its first geometry was 9 to 24 ms. A
/// third over sway's bound, so that a window placed within it never shows
/// on another output's bar.
pub const GRACE: Duration = Duration::from_millis(300);

/// Where a window is, as far as the taskbar knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// It has not yet reported an output; it was announced at this time.
    Unplaced {
        /// When it was announced.
        since: Instant,
    },
    /// Its known outputs: the last outputs it reported that were not none.
    Placed(BTreeSet<String>),
}

impl Placement {
    /// A window's placement once it reports `reported` at `now`, having
    /// been `before` (`None` for a window announced now): placed on what it
    /// reported, unless that is none, when what it was is kept.
    #[must_use]
    pub fn after(before: Option<&Self>, reported: BTreeSet<String>, now: Instant) -> Self {
        if !reported.is_empty() {
            return Self::Placed(reported);
        }
        before.cloned().unwrap_or(Self::Unplaced { since: now })
    }

    /// When its grace ends, for a window still unplaced.
    #[must_use]
    pub fn grace_ends(&self) -> Option<Instant> {
        match self {
            Self::Unplaced { since } => Some(*since + GRACE),
            Self::Placed(_) => None,
        }
    }
}

/// The bars there are: how many, and the names of their outputs (a bar on
/// an output with no name counted, not named).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bars {
    /// The names of the bars' outputs.
    pub named: BTreeSet<String>,
    /// How many bars there are.
    pub count: usize,
}

/// Whether a window at `placement` is on the bar of the output named `bar`
/// (`None` for an output with no name), among `bars`, at `now`.
#[must_use]
pub fn on_bar(placement: &Placement, bar: Option<&str>, bars: &Bars, now: Instant) -> bool {
    if bars.count <= 1 {
        return true;
    }
    match placement {
        Placement::Unplaced { since } => now >= *since + GRACE,
        Placement::Placed(known) => {
            let Some(bar) = bar.filter(|name| !name.is_empty()) else {
                return true;
            };
            known.contains(bar) || known.is_disjoint(&bars.named)
        }
    }
}

/// The buttons of `buttons` that the bar of the output named `bar` shows,
/// in their order, among `bars`, at `now` ([`on_bar`]).
#[must_use]
pub fn shown_on<Id: Clone>(
    buttons: &[Button<Id>],
    bar: Option<&str>,
    bars: &Bars,
    now: Instant,
) -> Vec<Button<Id>> {
    buttons
        .iter()
        .filter(|button| on_bar(&button.placement, bar, bars, now))
        .cloned()
        .collect()
}

/// The earliest end of a grace still running at `now`, among `buttons`'
/// unplaced windows: when to look again. `None` when no window is inside
/// its grace, which is when nothing needs a wake.
#[must_use]
pub fn next_deadline<Id>(buttons: &[Button<Id>], now: Instant) -> Option<Instant> {
    buttons
        .iter()
        .filter_map(|button| button.placement.grace_ends())
        .filter(|&end| end > now)
        .min()
}

#[cfg(test)]
mod tests;
