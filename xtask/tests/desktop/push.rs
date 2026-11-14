//! The pointer pushed into an edge or corner of an output, as a hand on a
//! mouse pushes it: for the edges tests.

use std::fmt::Debug;
use std::time::{Duration, Instant};

use deskrinch_test_input::Input;
use deskrinch_test_wait::{Deadline, waited_in_vain};

/// How long the pointer pushes against an edge or corner that must do
/// nothing: well past the time KWin waits there before it acts
/// (`ElectricBorderDelay`, 75 ms by default). The same negative claim as
/// the edges tests' `ACTS_WITHIN`, its look window.
pub(crate) const PUSHING: Duration = Duration::from_millis(1500);
/// How often a push repeats while it lasts, as a mouse's motion does.
const PUSH_EVERY: Duration = Duration::from_millis(20);

/// Brings the pointer to `(x, y)` on `output`, at an edge or corner of
/// its `(width, height)`; the motion that pushes on outwards from there.
fn bring_to(
    input: &mut Input,
    output: &str,
    (x, y): (f64, f64),
    (width, height): (f64, f64),
) -> (f64, f64) {
    // Outwards on each axis the point is at an edge of.
    let outwards = |at: f64, end: f64| {
        if at <= 0.0 {
            -4.0
        } else if at >= end - 1.0 {
            4.0
        } else {
            0.0
        }
    };
    input.move_pointer(output, x, y).unwrap();
    (outwards(x, width), outwards(y, height))
}

/// Takes the pointer back to the middle of `output`.
fn back_to_the_middle(input: &mut Input, output: &str, (width, height): (f64, f64)) {
    input
        .move_pointer(output, width / 2.0, height / 2.0)
        .unwrap();
}

/// Pushes the pointer into `point` on `output`, at an edge or corner that
/// must do nothing, as a hand on a mouse does: there, then on outwards in
/// small motions for [`PUSHING`], which the compositor stops at the edge.
pub(crate) fn push(input: &mut Input, output: &str, point: (f64, f64), size: (f64, f64)) {
    let (dx, dy) = bring_to(input, output, point, size);
    let pushing = Deadline::after(PUSHING);
    while !pushing.passed() {
        input.nudge_pointer(dx, dy).unwrap();
        std::thread::sleep(PUSH_EVERY);
    }
    back_to_the_middle(input, output, size);
}

/// Pushes the pointer into `point` on `output`, at an edge or corner that
/// must start something, until `started` sees it has, within the patience:
/// as a person keeps pushing until something happens. KWin acts only on a
/// pointer that has stayed at the edge for a while by the time stamps of
/// its motions, so a stalled compositor delays the result and cannot fail
/// it; an edge that never acts fails at the patience, saying how long it was
/// pushed, with how many motions, and what was last seen. What it found,
/// with how long it pushed and in how many motions, for the record.
#[track_caller]
pub(crate) fn push_until<T, S: Debug>(
    input: &mut Input,
    output: &str,
    (point, size): ((f64, f64), (f64, f64)),
    what: &str,
    mut started: impl FnMut() -> Result<T, S>,
) -> (T, Duration, u32) {
    let (dx, dy) = bring_to(input, output, point, size);
    let pushing = Deadline::patience();
    let since = Instant::now();
    let mut nudges = 0_u32;
    let found = loop {
        let last = match started() {
            Ok(found) => break found,
            Err(last) => last,
        };
        assert!(
            !pushing.passed(),
            "pushed for {:?}, in {nudges} motions: {}",
            since.elapsed(),
            waited_in_vain(what, &last)
        );
        input.nudge_pointer(dx, dy).unwrap();
        nudges += 1;
        std::thread::sleep(PUSH_EVERY);
    };
    let took = since.elapsed();
    back_to_the_middle(input, output, size);
    (found, took, nudges)
}
