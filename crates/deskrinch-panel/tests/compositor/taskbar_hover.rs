//! The button under the pointer is marked, on [`crate::taskbar`]'s fixture:
//! the pointer moved onto a button that is not the active one marks it in
//! the theme's hovered-option colour; moved off the panel, the mark goes.
//!
//! The mark is seen in the button's padding, left of its icon: the bar's own
//! colour (sampled in the bar's padding at its left end) before, and after
//! the pointer has left the panel. Under the pointer it differs from the
//! bar, and from the active window's button, whose mark is the theme's
//! selected-option colour. In the light scheme rinch's theme gives the two
//! the same colour (rinch issue 2727), so there the test asserts only that
//! the mark comes and goes.
//!
//! What each change costs is read from rinch's own counters on the panel's
//! frame lines ([`frames::hover_step`], [`frames::until_both_clean`]):
//!
//! - A step (the pointer onto the button, or off the panel) with any frame
//!   without reactive work (no `signal_notifies`) has exactly one, and it
//!   repaints and lays nothing out: what a `:hover` change costs.
//! - A step with reactive frames is crowded: drawn for something else the
//!   panel follows (the clock's minute, a window's title, an icon index
//!   landing late on a loaded machine), or the hover's restyle batched into
//!   one frame with such a change. The pointer goes on and off again until
//!   a clean step onto the button and a clean step off it have both been
//!   seen: one frame without reactive work, and no reactive frame.
//! - A hover that came to set a signal, on entering or on leaving, directly
//!   or deferred, crowds every step in its direction, and the test fails at
//!   the patience, naming that direction; the clock or a late arrival
//!   crowds a step or two.

use deskrinch_test_screen::Rgba;

use crate::frames::{self, Step, hover_step};
use crate::taskbar::{Bar, GREEN, RED, near};
use crate::taskbar_clicks::open_active;

/// How far apart, in some channel, two of the theme's flat colours must be
/// on the screen to count as different: more than a compositor's rounding.
/// Not the icons' [`near`] tolerance: the theme's hovered and selected
/// options are neighbouring shades (in the dark scheme, 7 apart per
/// channel), which that tolerance, made for telling icons apart, would
/// call the same.
const ROUNDING: u8 = 2;

/// The hover in the dark scheme, where the hovered and the selected option
/// differ: the mark differs from the bar and from the active mark.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_button_under_the_pointer_is_marked_until_it_leaves_dark() {
    hover("dark", true);
}

/// The hover in the light scheme: the mark comes and goes. That it differs
/// from the active mark is left out: rinch's theme gives the hovered and
/// the selected option the same colour there (rinch issue 2727).
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_button_under_the_pointer_is_marked_until_it_leaves_light() {
    hover("light", false);
}

/// The colours a step compares the button's against.
struct Colours {
    /// The bar's own.
    plain: Rgba,
    /// The active window's button's.
    active: Rgba,
    /// Whether the hovered and the selected option differ in the scheme.
    apart: bool,
}

/// Red's window, then green's, active; the pointer onto red's button, then
/// off the panel, again while no step has been free of reactive frames.
/// `apart` is whether the hovered and the selected option are different
/// colours in `scheme`.
fn hover(scheme: &str, apart: bool) {
    let mut bar = Bar::start_in(scheme);
    let _red = open_active(&bar, RED, "red");
    let _green = open_active(&bar, GREEN, "green");
    let red = bar.wait_button(RED);
    let green = bar.wait_button(GREEN);
    bar.point_off();
    let (plain, before, active) = bar.settled(|image| {
        (
            Bar::bar(image, red),
            Bar::beside(image, red),
            Bar::beside(image, green),
        )
    });
    let colours = Colours {
        plain: sample(plain),
        active: sample(active),
        apart,
    };
    let before = sample(before);
    assert!(
        near(before, colours.plain),
        "red's button is not the bar's colour before the pointer: {before:?} against {:?}",
        colours.plain
    );
    frames::until_both_clean(
        &mut bar,
        |bar| point_onto(bar, red, &colours),
        |bar| point_off(bar, red, &colours),
    );
}

/// The pointer onto red's button (its icon `red`): the mark comes. Returns
/// what the step was ([`hover_step`]).
fn point_onto(bar: &mut Bar, red: (u32, u32, u32, u32), colours: &Colours) -> Step {
    let from = frames::step(bar.panel());
    bar.point_at(red);
    bar.see("red's button marked under the pointer", |_, image| {
        Bar::beside(image, red).is_some_and(|p| !near(p, colours.plain))
    });
    let hovered = sample(bar.settled(|image| Bar::beside(image, red)));
    assert!(
        !near(hovered, colours.plain),
        "the mark is the bar's colour: {hovered:?} against {:?}",
        colours.plain
    );
    if colours.apart {
        assert!(
            apart_from(hovered, colours.active),
            "the mark is the active button's colour: {hovered:?} against {:?}",
            colours.active
        );
    }
    hover_step(bar.panel(), from, "onto red's button")
}

/// The pointer off the panel: red's mark goes. Returns what the step was
/// ([`hover_step`]).
fn point_off(bar: &mut Bar, red: (u32, u32, u32, u32), colours: &Colours) -> Step {
    let from = frames::step(bar.panel());
    bar.point_off();
    bar.see(
        "red's mark gone with the pointer off the panel",
        |_, image| Bar::beside(image, red).is_some_and(|p| near(p, colours.plain)),
    );
    let _ = bar.settled(|image| Bar::beside(image, red));
    hover_step(bar.panel(), from, "off the panel")
}

/// Whether `a` and `b` differ by more than [`ROUNDING`] in some colour
/// channel.
fn apart_from(a: Rgba, b: Rgba) -> bool {
    [(a.r, b.r), (a.g, b.g), (a.b, b.b)]
        .into_iter()
        .any(|(a, b)| a.abs_diff(b) > ROUNDING)
}

/// A sampled pixel, which is on the screen whenever its button is.
fn sample(pixel: Option<Rgba>) -> Rgba {
    pixel.expect("a pixel inside the panel")
}
