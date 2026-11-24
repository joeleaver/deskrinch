//! Each window draws its picture in every frame across a hand-over that
//! replaces its image: the picture's file replaced, then the fit changed,
//! each a new image under a new name in rinch's registry while the old is
//! let go.
//!
//! It holds rinch to resolving a registered `rinch-image:` source in the
//! frame that names it (rinch issue 2857). Before rinch did, a source a
//! document had not shown was loaded on a background thread, so the frame
//! in which a window's style moved to the new name drew the colour alone:
//! one such frame per hand-over, in all 54 passes of three runs over KWin,
//! sway and labwc (scale 1, scale 2, turned) with both renderers.

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
/// stretched to any output, its edge is a third of the way down.
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

/// Waits until every output shows the picture stretched: `top` at a sixth
/// of its height and `rest` at its middle.
fn wait_stretched(top: Rgba, rest: Rgba) {
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |image: &Image| {
            let (w, h) = (image.width(), image.height());
            let seen = [h / 6, h / 2].map(|y| image.pixel(w / 2, y).unwrap());
            let want = [top, rest];
            (!seen.iter().zip(want).all(|(&s, w)| about(s, w)))
                .then(|| format!("{seen:?}, waiting for {want:?}"))
        });
    }
}

/// Waits until every output shows the square picture contained: the colour
/// in its top left corner (a bar, whichever way the output is turned) and
/// the picture's `rest` at its middle.
fn wait_contained(rest: Rgba) {
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |image: &Image| {
            let (w, h) = (image.width(), image.height());
            let seen = [
                image.pixel(2, 2).unwrap(),
                image.pixel(w / 2, h / 2).unwrap(),
            ];
            let want = [BLUE, rest];
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
    wait_stretched(ORANGE, GREEN);
    let before = wallpaper.watch().lines().len();

    // A new picture: a new generation, every name new.
    let staged = dir.path().join("a.png.new");
    fs::write(&staged, thirds(GREEN, ORANGE)).unwrap();
    fs::rename(&staged, &picture).unwrap();
    wait_stretched(GREEN, ORANGE);
    // A new fit: every made-smaller image new.
    write_settings(&settings(&picture, "contain"));
    wait_contained(ORANGE);

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
