//! The one timer for the new windows' graces
//! ([`deskrinch_taskbar::on_bar`](mod@deskrinch_taskbar::on_bar)): a window
//! the compositor has not yet placed is on no bar until its grace is over,
//! and the bars must look again then.
//!
//! It lives on the UI's thread, beside the bars that read it: the buttons
//! come to the UI through their hand-over as before, and an effect over
//! them arms one rinch timeout for the earliest grace still running
//! ([`deskrinch_taskbar::next_deadline`]). When it fires, it sets the
//! [`GraceClock`] to that deadline, which every bar's list reads, and the
//! effect arms the next, if any. It is cleared when that window is placed
//! or closes first. With no window inside its grace, which is the usual
//! state, nothing is armed and nothing wakes the panel.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use deskrinch_taskbar::next_deadline;
use rinch::prelude::*;
use rinch::reactive::Effect;

use crate::widgets::taskbar::Buttons;

/// The time the bars judge the graces by: the end of the last grace that
/// ran out, or the panel's start. Only an unplaced window's button reads
/// it, so only a grace's end moves a bar.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GraceClock(pub(crate) Signal<Instant>);

/// Follows `buttons`' unplaced windows, keeping one timer for the earliest
/// grace still running, and returns the clock it moves.
pub(crate) fn follow(buttons: Signal<Buttons>) -> GraceClock {
    let clock = Signal::new(Instant::now());
    let armed: Rc<Cell<Option<(Instant, TimeoutHandle)>>> = Rc::default();
    Effect::new(move || {
        // Read so that a grace's end arms the next one.
        clock.get();
        let deadline = buttons.with(|buttons| next_deadline(buttons, Instant::now()));
        if armed.get().map(|(at, _)| at) == deadline {
            return;
        }
        if let Some((_, handle)) = armed.take() {
            clear_timeout(handle);
        }
        if let Some(at) = deadline {
            // Rounded up, so that it never fires before the deadline; the
            // clock is set to the deadline itself.
            let wait = at.saturating_duration_since(Instant::now());
            let millis = u32::try_from(wait.as_micros().div_ceil(1000)).unwrap_or(u32::MAX);
            let handle = set_timeout(millis, move || clock.set(at));
            armed.set(Some((at, handle)));
        }
    });
    GraceClock(clock)
}
