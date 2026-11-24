//! The button under the pointer is marked, on [`crate::taskbar`]'s fixture:
//! the pointer moved onto a button that is not the active one marks it in
//! the theme's hovered-option colour; moved off the panel, the mark goes.
//!
//! The mark is seen in the button's padding, left of its icon: the bar's own
//! colour (sampled in the bar's padding at its left end) before, and after
//! the pointer has left the panel. Under the pointer it differs from the
//! bar, and from the active window's button, whose mark is the theme's
//! selected-option colour, in both schemes.
//!
//! What each change costs is read from rinch's own counters on the panel's
//! frame lines, round by round ([`frames::until_clean_round`]):
//!
//! - A step (the pointer onto the button, or off the panel) with any frame
//!   without reactive work (no `signal_notifies` or `effect_runs`) has
//!   exactly one, and it repaints and lays nothing out: what a `:hover`
//!   change costs.
//! - A step with reactive frames is crowded: drawn for something else the
//!   panel follows (the clock's minute, a window's title, an icon index
//!   landing late on a loaded machine), or the hover's restyle batched into
//!   one frame with such a change.
//! - The pointer goes on and off again until a round after the first is
//!   clean in both its steps: one frame without reactive work each, and no
//!   reactive frame. The steps' windows are contiguous, so a frame between
//!   two steps is read too.
//! - A hover that came to set a signal, on entering or on leaving, directly
//!   or deferred by less than a round, crowds every round, and the test fails
//!   at the patience; the clock or a late arrival crowds a round or two. Work
//!   deferred by longer than a round is the rule's limit.

use deskrinch_test_screen::Rgba;

use crate::frames;
use crate::taskbar::{Bar, GREEN, RED, near};
use crate::taskbar_clicks::open_active;

/// How far apart, in some channel, two of the theme's flat colours must be
/// on the screen to count as different: more than a compositor's rounding.
/// Not the icons' [`near`] tolerance: the theme's hovered and selected
/// options may be neighbouring shades, which that tolerance, made for
/// telling icons apart, would call the same.
const ROUNDING: u8 = 2;

/// The hover in the dark scheme: the mark differs from the bar and from the
/// active mark.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_button_under_the_pointer_is_marked_until_it_leaves_dark() {
    hover("dark");
}

/// The hover in the light scheme: the mark differs from the bar and from
/// the active mark.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_button_under_the_pointer_is_marked_until_it_leaves_light() {
    hover("light");
}

/// The colours a step compares the button's against.
struct Colours {
    /// The bar's own.
    plain: Rgba,
    /// The active window's button's.
    active: Rgba,
}

/// Red's window, then green's, active; the pointer onto red's button, then
/// off the panel, round after round until one after the first is clean.
fn hover(scheme: &str) {
    let mut bar = Bar::start_in(scheme);
    let _red = open_active(&bar, RED, "red");
    let _green = open_active(&bar, GREEN, "green");
    let red = bar.wait_button(RED);
    let green = bar.wait_button(GREEN);
    // The active window's mark is read while green is active, before the
    // pointer goes to the bar's output: the windows are where the compositor
    // opened them, which may be another output, and sway gives the keyboard
    // to the workspace of an output the pointer enters (an empty one, then:
    // no window is active after `point_off` there). Waited for on a still
    // picture: red's mark going and green's coming are reported in turn.
    let active = bar.judged("green's button marked as the active one", |_, image| {
        let plain = sample(Bar::bar(image, green));
        let active = sample(Bar::beside(image, green));
        if near(active, plain) {
            Err(format!(
                "green's button is not marked: {active:?} against the bar's {plain:?}"
            ))
        } else {
            Ok(active)
        }
    });
    bar.point_off();
    // Red's button has the bar's colour before the pointer: a change that
    // comes about, which a compositor may report a while after green has
    // the keyboard (sway, on a pass, later than the still gap). It is waited
    // for on a still picture, whose bar colour is then the one compared with.
    let plain = bar.judged(
        "red's button the bar's colour before the pointer",
        |_, image| {
            let plain = sample(Bar::bar(image, red));
            let before = sample(Bar::beside(image, red));
            if near(before, plain) {
                Ok(plain)
            } else {
                Err(format!(
                    "red's button is not the bar's colour before the pointer: {before:?} against \
                     {plain:?}"
                ))
            }
        },
    );
    let colours = Colours { plain, active };
    let window = bar.panel().window_on(bar.output());
    let from = frames::step(bar.panel());
    frames::until_clean_round(
        &mut bar,
        Bar::panel,
        &window,
        |bar| point_onto(bar, red, &colours),
        |bar| point_off(bar, red, &colours),
    );
    // The hover was on one bar: no other bar drew for it (one that drew at
    // all drew with reactive work, the panel's own state: a clock's minute).
    for (other, output) in bar.panel().bars() {
        if other == window {
            continue;
        }
        let drawn = frames::drawn_without_reactive_work_since(bar.panel(), &other, from);
        assert!(
            drawn.is_empty(),
            "{output}'s bar drew for a hover on another bar: {drawn:#?}"
        );
    }
}

/// The pointer onto red's button (its icon `red`): returns once the mark
/// has come and the screen keeps still.
fn point_onto(bar: &mut Bar, red: (u32, u32, u32, u32), colours: &Colours) {
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
    assert!(
        apart_from(hovered, colours.active),
        "the mark is the active button's colour: {hovered:?} against {:?}",
        colours.active
    );
}

/// The pointer off the panel: returns once red's mark has gone and the
/// screen keeps still.
fn point_off(bar: &mut Bar, red: (u32, u32, u32, u32), colours: &Colours) {
    bar.point_off();
    bar.see(
        "red's mark gone with the pointer off the panel",
        |_, image| Bar::beside(image, red).is_some_and(|p| near(p, colours.plain)),
    );
    let _ = bar.settled(|image| Bar::beside(image, red));
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
