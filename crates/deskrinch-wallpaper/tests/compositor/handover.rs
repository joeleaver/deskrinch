//! Each window draws its picture in every frame across a hand-over that
//! replaces its image: the picture's file replaced, then the fit changed,
//! each a new image under a new name in rinch's registry while the old is
//! let go.
//!
//! It fails today. rinch loads a source a document has not shown before on
//! a background thread, so the frame in which a window's style moves to the
//! new name paints before the picture has come, and draws the colour alone:
//! one such frame per hand-over, in 35 of 36 passes over KWin, sway and
//! labwc with both renderers. rinch issue 2857: a registered `rinch-image:`
//! source resolved in the frame that names it, with no thread hop; this
//! test is what it must pass.

use std::fs;
use std::path::Path;

use deskrinch_test_component::counter;
use deskrinch_test_pictures::png::{BitDepth, ColorType, encode};
use deskrinch_test_screen::{Image, Rgba, Screen};

use crate::support::{
    BLUE, ORANGE, Wallpaper, about, in_nested_session, wait_output, write_settings, written,
};

const GREEN: Rgba = Rgba::opaque(0x1f, 0xb0, 0x4a);

/// A side larger than any output of the passes this runs in, so that each
/// output is handed an image made smaller for it, named by its size and
/// fit.
const SIDE: u32 = 2000;

/// A `SIDE` square PNG whose top third is `top` and the rest `rest`:
/// stretched to a wide output its edge is a third of the way down, covered
/// a fifth of the way down.
fn thirds(top: Rgba, rest: Rgba) -> Vec<u8> {
    let pixels: Vec<u8> = (0..SIDE)
        .flat_map(|y| {
            let color = if y < SIDE / 3 { top } else { rest };
            (0..SIDE).flat_map(move |_| [color.r, color.g, color.b, 255])
        })
        .collect();
    encode(SIDE, SIDE, ColorType::Rgba, BitDepth::Eight, &pixels)
}

fn settings(picture: &Path, fit: &str) -> String {
    format!(
        "[wallpaper]\ncolor = \"{}\"\nimage = \"{}\"\nfit = \"{fit}\"\n",
        written(BLUE),
        picture.display()
    )
}

/// Waits until every output shows `top` at a sixth of its height and `rest`
/// at its middle, and, at 27% of its height, `at_27` (`top` stretched,
/// `rest` covered).
fn wait_shown(top: Rgba, rest: Rgba, at_27: Rgba) {
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |image: &Image| {
            let (w, h) = (image.width(), image.height());
            let seen = [h / 6, h * 27 / 100, h / 2].map(|y| image.pixel(w / 2, y).unwrap());
            let want = [top, at_27, rest];
            (!seen.iter().zip(want).all(|(&s, w)| about(s, w)))
                .then(|| format!("{seen:?}, waiting for {want:?}"))
        });
    }
}

/// The window number of a frame line (`… window N frame M: …`).
fn window_of(line: &str) -> Option<&str> {
    let rest = &line[line.find("window ")? + "window ".len()..];
    let (number, rest) = rest.split_once(' ')?;
    rest.starts_with("frame ").then_some(number)
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn every_frame_draws_the_picture_while_a_hand_over_replaces_it() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("a.png");
    fs::write(&picture, thirds(ORANGE, GREEN)).unwrap();
    write_settings(&settings(&picture, "stretch"));
    let wallpaper = Wallpaper::start();
    wait_shown(ORANGE, GREEN, ORANGE);
    let before = wallpaper.watch().lines().len();

    // A new picture: a new generation, every name new.
    let staged = dir.path().join("a.png.new");
    fs::write(&staged, thirds(GREEN, ORANGE)).unwrap();
    fs::rename(&staged, &picture).unwrap();
    wait_shown(GREEN, ORANGE, GREEN);
    // A new fit: every made-smaller image new.
    write_settings(&settings(&picture, "cover"));
    wait_shown(GREEN, ORANGE, ORANGE);

    let lines = wallpaper.watch().lines();
    let blank: Vec<&String> = lines[before..]
        .iter()
        // A frame that painted: a frame rinch skipped (nothing dirty) keeps
        // what is on the screen.
        .filter(|line| window_of(line).is_some() && counter(line, "paint_frames").is_some())
        .filter(|line| counter(line, "image_draws").is_none())
        .collect();
    assert!(
        blank.is_empty(),
        "frames that drew no picture across the hand-overs:\n{}",
        blank
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
