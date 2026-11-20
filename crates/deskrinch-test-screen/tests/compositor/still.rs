//! A picture judged once the screen is still: what a judgment that
//! something stays must see, where the first capture after an event can
//! come before the event's change reaches the screen.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use deskrinch_test_client::{Compositor, Paint, TestClient};
use deskrinch_test_screen::{Image, Pictures, Rgba, Screen, still_gap};

use crate::support::fullscreen;

/// A colour no compositor draws by itself.
const ORANGE: Rgba = Rgba::opaque(0xe0, 0x7a, 0x1f);

/// The least gap the test asks for: long against any stall of the machine.
const GAP: Duration = Duration::from_secs(1);

/// The gap the test judges over: [`GAP`], or the compositor's own still gap
/// where that is longer.
fn gap() -> Duration {
    GAP.max(still_gap())
}

/// Pixels this far from an output's edges are left out of [`covered`]: a
/// window drawn at buffer scale 1 is enlarged on an output at scale 2, and
/// the compositor filters its edges with what is below (labwc does).
const EDGE: u32 = 4;

/// How far a channel may be from the colour drawn: compositors may round
/// when they composite.
const ROUNDING: u8 = 2;

/// Whether `image` is `color`, within [`ROUNDING`], but for [`EDGE`]
/// pixels at its edges: the window covers the output.
fn covered(image: &Image, color: Rgba) -> bool {
    let close = |got: Rgba| {
        [(got.r, color.r), (got.g, color.g), (got.b, color.b)]
            .iter()
            .all(|&(g, w)| g.abs_diff(w) <= ROUNDING)
    };
    (EDGE..image.height().saturating_sub(EDGE)).all(|y| {
        (EDGE..image.width().saturating_sub(EDGE)).all(|x| close(image.pixel(x, y).unwrap()))
    })
}

/// A window covers an output in one colour. Told to, its client closes it
/// a moment later, and the picture judged once the screen is still, asked
/// for at once, is the window gone.
///
/// Ordered by cause, so that a right [`Pictures::judged_over`] passes
/// however late any capture is: the orange picture is seen before the
/// client is told anything, and what the judgment is handed is not orange
/// whether the first look comes before the close or after it.
///
/// The gap is [`GAP`], a second, or the compositor's own still gap where
/// that is longer, and the client closes its window a quarter of it after
/// it is told. The close must reach the screen before the second of two
/// captures a gap apart, or the orange screen would rightly be called
/// still: three quarters of a second for a thread to wake, a request to
/// cross and a compositor to draw (KWin's close animation included), so
/// that a right judgment fails only if the machine stops for that long.
/// The quarter second decides how surely the mutants are caught: one
/// capture in place of a still picture comes a few milliseconds after the
/// client is told, and the default gap in place of the one asked for
/// (100 ms under sway and labwc) ends before the close.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_change_that_comes_a_moment_after_an_event_is_seen_once_the_screen_is_still() {
    Compositor::current();
    let mut screen = Screen::connect().unwrap();
    let output = screen.outputs().remove(1);
    let (drawn, on_screen) = mpsc::channel();
    let (go, told) = mpsc::channel::<()>();
    let at = output.clone();
    let client = thread::spawn(move || {
        let client = TestClient::connect().unwrap();
        let shown = fullscreen(&client, &at, Paint::Solid(ORANGE));
        drawn.send(()).unwrap();
        told.recv().unwrap();
        thread::sleep(gap() / 4);
        // The window, then the connection, which sends what is left.
        drop(shown);
        drop(client);
    });
    on_screen.recv().unwrap();
    // The window up and the screen still, before the client is told
    // anything: this cannot race.
    Pictures::judged(&mut screen, &output, "the window to cover it", |image| {
        covered(image, ORANGE)
            .then_some(())
            .ok_or("not covered in orange")
    });

    go.send(()).unwrap();
    let judged = Pictures::judged_over(
        &mut screen,
        &output,
        gap(),
        "the output once still",
        |image| Ok::<_, ()>(image.clone()),
    );
    assert!(
        !covered(&judged, ORANGE),
        "the picture judged once still is the window's, which its client closed"
    );
    client.join().unwrap();
}
