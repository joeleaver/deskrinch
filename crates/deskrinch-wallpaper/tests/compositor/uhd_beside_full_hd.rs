//! The pass with outputs of its own (`cargo xtask compositor-tests`' row
//! for this module): a 3840x2160 output beside a 1920x1080 one. What the
//! picture's per-output pixels are when the outputs differ in size, which
//! the default pass's two equal outputs cannot show.
//!
//! - A picture larger than both outputs is made smaller in advance for each
//!   (two needs, the two together smaller than the picture): each output is
//!   drawn one device pixel per pixel at its own size.
//! - A picture the size of the larger output is needed itself there, so
//!   every output takes it (what is held is never more than the picture):
//!   the larger output shows it one pixel per pixel, the smaller shows it
//!   scaled by rinch at paint.

use deskrinch_picture::{Fit, Formats, decode_file, fit};
use deskrinch_test_client::require_outputs;
use deskrinch_test_screen::{Image, Rgba, Screen};

use crate::exact::{ROOMY, first_different, settings, write_blocks_of};
use crate::support::{Wallpaper, about, in_nested_session, wait_output, write_settings};

/// The pass's outputs, the larger first.
const OUTPUTS: [(i32, i32); 2] = [(3840, 2160), (1920, 1080)];

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_larger_than_both_outputs_is_drawn_one_pixel_per_pixel_on_each() {
    in_nested_session();
    require_outputs(&OUTPUTS);
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("blocks.png");
    write_blocks_of(&picture, 5120, 2880, 13);
    let decoded = decode_file(&picture, Formats::PNG, &ROOMY).unwrap().image;
    write_settings(&settings(&picture, "cover"));
    let _wallpaper = Wallpaper::start();
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |shown| {
            let (w, h) = (shown.width(), shown.height());
            let expected = fit(&decoded, w, h, Fit::Cover, &ROOMY).unwrap();
            first_different(shown, (0, 0, w, h), expected.rgba())
        });
    }
}

/// The first block of `shown` whose centre is not the colour of the same
/// block's centre in `expected` (`width` wide), with where: `side`-pixel
/// blocks as they are shown, scaled from the picture's.
fn first_block_different(shown: &Image, expected: &[u8], side: u32) -> Option<String> {
    let (w, h) = (shown.width(), shown.height());
    for y in (side / 2..h).step_by(side as usize) {
        for x in (side / 2..w).step_by(side as usize) {
            let at = ((y * w + x) * 4) as usize;
            let want = Rgba::opaque(expected[at], expected[at + 1], expected[at + 2]);
            let got = shown.pixel(x, y).unwrap();
            if !about(got, want) {
                return Some(format!("({x}, {y}) is {got:?}, not {want:?}"));
            }
        }
    }
    None
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_the_size_of_the_larger_output_is_the_picture_itself_on_both() {
    in_nested_session();
    require_outputs(&OUTPUTS);
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("blocks.png");
    // Blocks of 64 pixels: 32 on the smaller output, whose centres any
    // scaling filter leaves the block's colour.
    write_blocks_of(&picture, 3840, 2160, 64);
    let decoded = decode_file(&picture, Formats::PNG, &ROOMY).unwrap().image;
    write_settings(&settings(&picture, "cover"));
    let _wallpaper = Wallpaper::start();
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |shown| {
            let (w, h) = (shown.width(), shown.height());
            if (w, h) == (3840, 2160) {
                // The picture itself, one pixel per pixel.
                first_different(shown, (0, 0, w, h), decoded.rgba())
            } else {
                // The picture scaled at paint: each block's centre.
                let expected = fit(&decoded, w, h, Fit::Cover, &ROOMY).unwrap();
                first_block_different(shown, expected.rgba(), 32)
            }
        });
    }
}
