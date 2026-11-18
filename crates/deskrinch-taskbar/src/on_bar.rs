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
//! - A placed window is on the bars `deskrinch-bars` gives for its known
//!   outputs ([`deskrinch_bars::outputs_on_bar`], whose documentation has
//!   the rule and why each case is shown where it is).
//! - With several bars ([`on_bar`]):
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
//! The time is handed in, never read here. Whether a grace is over is
//! judged by one clock alone, the time handed to [`on_bar`] as `now`: the
//! panel wakes for the earliest grace that has not ended by that clock
//! ([`next_wake`]), at once when it has already ended in real time, and on
//! waking moves the clock to the later of that end and the real time
//! ([`clock_on_wake`]), so that every grace that has run out by then is
//! over together. It wakes only while some window is unplaced and its
//! grace not yet over by the clock.
//!
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

use deskrinch_bars::{Bars, outputs_on_bar};

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

/// Whether a window at `placement` is on the bar of the output named `bar`
/// (`None` for an output with no name), among `bars`, at `now`.
#[must_use]
pub fn on_bar(placement: &Placement, bar: Option<&str>, bars: &Bars, now: Instant) -> bool {
    match placement {
        Placement::Unplaced { since } => bars.count <= 1 || now >= *since + GRACE,
        Placement::Placed(known) => outputs_on_bar(known, bar, bars),
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

/// When the panel next looks at the graces ([`next_wake`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wake {
    /// The end of the grace it wakes for.
    pub at: Instant,
    /// How long from the real time until then: none when it has passed.
    pub after: Duration,
}

/// The earliest end, among `buttons`' unplaced windows, of a grace not yet
/// over by `clock` (the time the bars judge the graces by), and how long
/// from `now`, the real time, until it. `None` when every grace is over by
/// the clock, or no window is unplaced: then nothing needs a wake, and the
/// clock stays as it is.
#[must_use]
pub fn next_wake<Id>(buttons: &[Button<Id>], clock: Instant, now: Instant) -> Option<Wake> {
    let at = buttons
        .iter()
        .filter_map(|button| button.placement.grace_ends())
        .filter(|&end| end > clock)
        .min()?;
    Some(Wake {
        at,
        after: at.saturating_duration_since(now),
    })
}

/// The clock once the wake for the grace ending at `at` comes, at `now`,
/// the real time: the later of the two, so that every grace that has run
/// out by then is over together, the one woken for included even if the
/// wake came early.
#[must_use]
pub fn clock_on_wake(at: Instant, now: Instant) -> Instant {
    at.max(now)
}

#[cfg(test)]
mod tests;
