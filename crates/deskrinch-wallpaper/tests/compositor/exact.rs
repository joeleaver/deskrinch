//! A picture larger than its output is drawn one device pixel per pixel:
//! what the screen shows is the picture fitted by `deskrinch-picture` at the
//! output's size, not resampled again by rinch.
//!
//! The picture is blocks of colour with sharp edges, so that any second
//! resampling (a half-pixel shift, a scale a pixel off) blurs an edge and
//! shows. The expected pixels are made here the way the wallpaper makes
//! them, from the same file: [`deskrinch_picture::fit`] at the size the
//! output is captured at, which is its device size in every pass (scale 1,
//! scale 2, turned).

use std::cell::RefCell;
use std::path::Path;

use deskrinch_picture::{Fit, Formats, Limits, decode_file, fit};
use deskrinch_test_screen::{Image, Rgba, Screen};

use crate::support::{
    BLUE, Wallpaper, about, in_nested_session, wait_output, write_settings, written,
};

/// Generous limits for the test's own decode.
pub(crate) const ROOMY: Limits = Limits {
    max_encoded_bytes: 256 << 20,
    max_side: 16_384,
    max_decoded_bytes: 512 << 20,
    max_working_bytes: 1 << 30,
};

/// Writes a `width` × `height` picture of 13-pixel blocks of colour, as a
/// PNG at `path`.
fn write_blocks(path: &Path, width: u32, height: u32) {
    write_blocks_of(path, width, height, 13);
}

/// Writes a `width` × `height` picture of `side`-pixel blocks of colour, as
/// a PNG at `path`.
pub(crate) fn write_blocks_of(path: &Path, width: u32, height: u32, side: u32) {
    let picture = image::RgbaImage::from_fn(width, height, |x, y| {
        let (bx, by) = (x / side, y / side);
        let n = bx.wrapping_mul(2_654_435_761) ^ by.wrapping_mul(40_503);
        let [r, g, b, _] = n.to_le_bytes();
        image::Rgba([r, g, b, 255])
    });
    picture.save(path).unwrap();
}

/// The first pixel of `shown`, within `area` (x, y, width, height), that
/// is not `expected`'s at the same place in the area, with where.
pub(crate) fn first_different(
    shown: &Image,
    area: (u32, u32, u32, u32),
    expected: &[u8],
) -> Option<String> {
    let (ax, ay, aw, ah) = area;
    for y in 0..ah {
        for x in 0..aw {
            let at = ((y * aw + x) * 4) as usize;
            let want = Rgba::opaque(expected[at], expected[at + 1], expected[at + 2]);
            let got = shown.pixel(ax + x, ay + y).unwrap();
            if !about(got, want) {
                return Some(format!("({}, {}) is {got:?}, not {want:?}", ax + x, ay + y));
            }
        }
    }
    None
}

/// The picture fitted as the wallpaper fits it, made once per size: a check
/// runs on every capture until the screen passes, and fitting a large
/// picture again on each, unoptimised, cost seconds an attempt.
pub(crate) struct Expected<'a> {
    decoded: &'a deskrinch_untrusted::Image,
    fit: Fit,
    made: RefCell<Option<((u32, u32), deskrinch_untrusted::Image)>>,
}

impl<'a> Expected<'a> {
    /// `decoded` fitted with `fit`, at whatever size it is asked for.
    pub(crate) fn new(decoded: &'a deskrinch_untrusted::Image, fit: Fit) -> Self {
        Self {
            decoded,
            fit,
            made: RefCell::new(None),
        }
    }

    /// The fitted picture's pixels at `width` × `height`, made the first time
    /// that size is asked for.
    pub(crate) fn rgba(&self, width: u32, height: u32) -> std::cell::Ref<'_, [u8]> {
        let size = (width, height);
        if self
            .made
            .borrow()
            .as_ref()
            .is_none_or(|(at, _)| *at != size)
        {
            let image = fit(self.decoded, width, height, self.fit, &ROOMY).unwrap();
            *self.made.borrow_mut() = Some((size, image));
        }
        std::cell::Ref::map(self.made.borrow(), |made| {
            made.as_ref()
                .map(|(_, image)| image.rgba())
                .unwrap_or_default()
        })
    }
}

pub(crate) fn settings(picture: &Path, fit: &str) -> String {
    format!(
        "[wallpaper]\ncolor = \"{}\"\nimage = \"{}\"\nfit = \"{fit}\"\n",
        written(BLUE),
        picture.display()
    )
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_covering_a_smaller_output_is_drawn_one_pixel_per_pixel() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("blocks.png");
    write_blocks(&picture, 3001, 2003);
    let decoded = decode_file(&picture, Formats::PNG, &ROOMY).unwrap().image;
    write_settings(&settings(&picture, "cover"));
    let _wallpaper = Wallpaper::start();
    for name in &Screen::connect().unwrap().outputs() {
        let expected = Expected::new(&decoded, Fit::Cover);
        wait_output(name, |shown| {
            let (w, h) = (shown.width(), shown.height());
            first_different(shown, (0, 0, w, h), &expected.rgba(w, h))
        });
    }
}

/// The part of `shown` that is not the colour: Contain's picture between
/// its bars, as (x, y, width, height).
fn inner_box(shown: &Image) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (shown.width(), shown.height());
    let picture = |x: u32, y: u32| !about(shown.pixel(x, y).unwrap(), BLUE);
    let top = (0..h).find(|&y| picture(w / 2, y))?;
    let bottom = (0..h).rev().find(|&y| picture(w / 2, y))?;
    let left = (0..w).find(|&x| picture(x, h / 2))?;
    let right = (0..w).rev().find(|&x| picture(x, h / 2))?;
    Some((left, top, right - left + 1, bottom - top + 1))
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_contained_in_a_smaller_output_is_drawn_one_pixel_per_pixel() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("blocks.png");
    // Wider than any output's shape, so there are bars above and below (or,
    // turned, wider bars).
    write_blocks(&picture, 4000, 1000);
    let decoded = decode_file(&picture, Formats::PNG, &ROOMY).unwrap().image;
    write_settings(&settings(&picture, "contain"));
    let _wallpaper = Wallpaper::start();
    for name in &Screen::connect().unwrap().outputs() {
        let expected = Expected::new(&decoded, Fit::Stretch);
        wait_output(name, |shown| {
            let Some(area) = inner_box(shown) else {
                return Some("no picture between the bars".to_owned());
            };
            let (x, y, w, h) = area;
            if x != 0 && y != 0 {
                return Some(format!("bars on both axes: {area:?}"));
            }
            first_different(shown, area, &expected.rgba(w, h))
        });
    }
}
