//! Two outputs at one position: the wallpaper goes on following its
//! settings while the compositor does not show one of its windows, and
//! both outputs show the last settings once apart again.
//!
//! This is rinch issue 2669's case as we met it. With the GPU renderer, a
//! present on a surface the compositor has stopped showing waited for a
//! frame callback that never came, which blocked the program's main thread,
//! every window and all its event handling, until the surface was shown
//! again. sway with two outputs at one position shows the wallpaper of only
//! one of them; KWin paces with `wp_fifo_v1` and was never affected.
//!
//! The test is a sequence of steps, each confirmed before the next is
//! written, because which present blocks is not certain: Mesa waits for
//! the previous frame's callback, so the first present on the hidden
//! window may go through and a later one block; the shown window may be
//! painted before the hidden one; and two edits written close together
//! merge in the settings watcher's pause and are handled as one. Each step
//! is a colour, seen on the output whose wallpaper is shown before the
//! next is written, and each repaints both windows. Four leave room for
//! the first presents to go through and still make several that would
//! block.

use deskrinch_test_client::{move_output, outputs};
use deskrinch_test_screen::{Rgba, Screen};
use deskrinch_test_wait::wait_for;

use crate::support::{
    BLUE, Wallpaper, first_other, in_nested_session, wait_covered, wait_output, write_settings,
    written,
};

/// The colours written in turn: none a compositor's own background, and
/// each unlike the one before.
const STEPS: [Rgba; 4] = [
    Rgba::opaque(0xd6, 0x24, 0x5c),
    Rgba::opaque(0x2a, 0xb0, 0x4e),
    Rgba::opaque(0xe0, 0x7a, 0x1f),
    Rgba::opaque(0x7b, 0x3f, 0xc4),
];

/// What a step last saw, with the wallpaper's last lines, printed as it is
/// (a `String`'s `Debug` would escape the lines' breaks).
struct Seen(String);

impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn settings(color: Rgba) -> String {
    format!("[wallpaper]\ncolor = \"{}\"\n", written(color))
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn with_two_outputs_at_one_position_it_follows_its_settings_and_shows_them_apart() {
    in_nested_session();
    write_settings(&settings(BLUE));
    let wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let layout = outputs().unwrap();
    assert_eq!(layout.len(), 2, "two outputs: {layout:?}");
    let together = move_output(&layout[1].name, layout[0].position).unwrap();

    // Which output the compositor still shows the wallpaper on: the one
    // that takes the first step's colour.
    write_settings(&settings(STEPS[0]));
    let names: Vec<String> = layout.iter().map(|output| output.name.clone()).collect();
    let shown = wait_for("an output to show the first step's colour", || {
        let mut screen = Screen::connect().unwrap();
        names
            .iter()
            .find(|name| first_other(&screen.capture(name).unwrap(), STEPS[0]).is_none())
            .cloned()
            .ok_or_else(|| {
                Seen(format!(
                    "neither of {names:?} shows it; the wallpaper's last lines:\n{}",
                    wallpaper.log()
                ))
            })
    });
    for (step, color) in (2..).zip(&STEPS[1..]) {
        write_settings(&settings(*color));
        let what = format!("step {step} of {}: {shown} to show {color:?}", STEPS.len());
        wait_for(&what, || {
            let image = Screen::connect().unwrap().capture(&shown).unwrap();
            first_other(&image, *color).map_or(Ok(()), |at| {
                Err(Seen(format!(
                    "{shown} has {at:?}: the wallpaper's main thread stopped; its last lines:\n{}",
                    wallpaper.log()
                )))
            })
        });
    }

    together.restore().unwrap();
    let last = STEPS[STEPS.len() - 1];
    for name in &names {
        wait_output(name, |image| {
            first_other(image, last).map(|at| format!("apart again, not the last colour: {at:?}"))
        });
    }
    wallpaper.assert_no_error();
}
