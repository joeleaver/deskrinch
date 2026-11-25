//! The one timer for the new windows' graces
//! ([`deskrinch_taskbar::on_bar`](mod@deskrinch_taskbar::on_bar)): a window
//! the compositor has not yet placed is on no bar until its grace is over,
//! and the bars must look again then.
//!
//! It lives on the UI's thread, beside the bars that read it: the buttons
//! come to the UI through their hand-over as before, and an effect over
//! them, the bars there are and the [`GraceClock`] arms one rinch timeout
//! for the earliest grace not yet over by that clock
//! ([`deskrinch_taskbar::next_wake`]). When it fires, it moves the clock
//! ([`deskrinch_taskbar::clock_on_wake`]), which every bar's list reads,
//! and the effect arms the next, if any. It is cleared when that window is
//! placed or closes first. Whether a grace is over is the clock's alone to
//! say, here and in the bars. With every window placed, which is the usual
//! state, or with one bar, where no window waits a grace, nothing is armed
//! and nothing wakes the panel.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use deskrinch_bars::Bars;
use deskrinch_taskbar::{Wake, clock_on_wake, next_wake};
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::widgets::taskbar::Buttons;

/// The time the bars judge the graces by: the panel's start, then at each
/// wake the later of the grace's end it woke for and the real time. Only an
/// unplaced window's button reads it, so only a grace's end moves a bar.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GraceClock(pub(crate) Signal<Instant>);

/// Follows `buttons`' unplaced windows among `bars`, keeping one timer for
/// the earliest grace not yet over by the clock, and returns the clock it
/// moves.
pub(crate) fn follow(buttons: Signal<Buttons>, bars: Memo<Bars>) -> GraceClock {
    let clock = Signal::new(Instant::now());
    let armed: Rc<Cell<Option<(Instant, TimeoutHandle)>>> = Rc::default();
    Effect::new(move || {
        let judged = clock.get();
        let bars = bars.get();
        let wake = buttons.with(|buttons| next_wake(buttons, &bars, judged, Instant::now()));
        if armed.get().map(|(at, _)| at) == wake.map(|wake| wake.at) {
            return;
        }
        if let Some((_, handle)) = armed.take() {
            clear_timeout(handle);
        }
        if let Some(Wake { at, after }) = wake {
            // Rounded up; a wake that comes early is no harm, since the
            // clock goes to the end it woke for.
            let millis = u32::try_from(after.as_micros().div_ceil(1000)).unwrap_or(u32::MAX);
            let handle = set_timeout(millis, move || clock.set(clock_on_wake(at, Instant::now())));
            armed.set(Some((at, handle)));
        }
    });
    GraceClock(clock)
}
