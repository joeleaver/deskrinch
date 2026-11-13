//! What the wallpaper shows: its colour on every output, followed live, and
//! a picture fitted over it.

use std::path::Path;

use deskrinch_test_screen::{Image, Rgba};

use crate::support::{
    BLUE, ORANGE, Wallpaper, about, in_nested_session, wait_covered, wait_output, write_settings,
    written,
};

/// The right half of the test picture.
const GREEN: Rgba = Rgba::opaque(0x1f, 0xb0, 0x4a);

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn covers_every_output_with_its_colour() {
    in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let _wallpaper = Wallpaper::start();
    wait_covered(BLUE);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_edit_of_the_settings_repaints_every_output() {
    in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let _wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(ORANGE)));
    wait_covered(ORANGE);
}

/// Writes a picture twice as wide as it is high, its left half [`ORANGE`]
/// and its right half [`GREEN`], as a PNG at `path`.
fn write_picture(path: &Path) {
    let picture = image::RgbaImage::from_fn(200, 100, |x, _| {
        let color = if x < 100 { ORANGE } else { GREEN };
        image::Rgba([color.r, color.g, color.b, 255])
    });
    picture.save(path).unwrap();
}

/// What is wrong with `image` for the test picture over [`BLUE`] with
/// `fit`, if anything. The picture is wider than any output (2:1, against
/// 16:9 or, turned, 9:16), so `contain` leaves the colour above and below
/// it; `stretch` and `contain` both put its halves on the output's halves.
fn wrong_fit(image: &Image, contained: bool) -> Option<String> {
    let (width, height) = (image.width(), image.height());
    let at = |x: u32, y: u32| image.pixel(x, y).unwrap();
    let mut checks = vec![
        ("the left half", at(width / 4, height / 2), ORANGE),
        ("the right half", at(width * 3 / 4, height / 2), GREEN),
    ];
    if contained {
        checks.push(("above the picture", at(width / 2, 1), BLUE));
        checks.push(("below the picture", at(width / 2, height - 2), BLUE));
    } else {
        checks.push(("the top left", at(1, 1), ORANGE));
        checks.push(("the bottom right", at(width - 2, height - 2), GREEN));
    }
    checks
        .into_iter()
        .find(|(_, pixel, wanted)| !about(*pixel, *wanted))
        .map(|(place, pixel, wanted)| format!("{place} is {pixel:?}, not {wanted:?}"))
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_is_fitted_to_every_output() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("halves.png");
    write_picture(&picture);
    let table = |fit: &str| {
        format!(
            "[wallpaper]\ncolor = \"{}\"\nimage = \"{}\"\nfit = \"{fit}\"\n",
            written(BLUE),
            picture.display()
        )
    };
    write_settings(&table("stretch"));
    let _wallpaper = Wallpaper::start();
    let outputs = deskrinch_test_screen::Screen::connect().unwrap().outputs();
    for name in &outputs {
        wait_output(name, |image| wrong_fit(image, false));
    }
    write_settings(&table("contain"));
    for name in &outputs {
        wait_output(name, |image| wrong_fit(image, true));
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_whose_name_a_url_would_change_is_shown() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    // A space, a fragment, an escape and a letter a URL would encode.
    let picture = dir.path().join("a b#c%41é.png");
    write_picture(&picture);
    write_settings(&format!(
        "[wallpaper]\ncolor = \"{}\"\nimage = \"{}\"\nfit = \"stretch\"\n",
        written(BLUE),
        picture.display()
    ));
    let _wallpaper = Wallpaper::start();
    for name in &deskrinch_test_screen::Screen::connect().unwrap().outputs() {
        wait_output(name, |image| wrong_fit(image, false));
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_that_cannot_be_shown_leaves_the_colour_and_says_so() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.png");
    write_settings(&format!(
        "[wallpaper]\ncolor = \"{}\"\nimage = \"{}\"\n",
        written(BLUE),
        missing.display()
    ));
    let wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    wallpaper.wait_said(&format!("cannot show {}", missing.display()));
}
