//! The picture's file while the wallpaper runs (replaced whole, rewritten
//! in place, damaged, taken away), a picture over the wallpaper's limits,
//! and what the wallpaper does to a picture before it is shown (its EXIF
//! orientation applied, its colour profile converted).

use std::fs;
use std::path::Path;

use deskrinch_test_pictures::png::{BitDepth, ColorType, encode, with_chunk, with_profile};
use deskrinch_test_pictures::{icc, jpeg};
use deskrinch_test_screen::{Image, Rgba, Screen};

use crate::support::{
    BLUE, ORANGE, Wallpaper, about, in_nested_session, stays_covered, stays_output, wait_covered,
    wait_output, write_settings, written,
};

const GREEN: Rgba = Rgba::opaque(0x1f, 0xb0, 0x4a);

/// A `width` × `height` PNG all `color`.
fn solid(width: u32, height: u32, color: Rgba) -> Vec<u8> {
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|_| [color.r, color.g, color.b, 255])
        .collect();
    encode(width, height, ColorType::Rgba, BitDepth::Eight, &pixels)
}

fn stretched(picture: &Path) -> String {
    format!(
        "[wallpaper]\ncolor = \"{}\"\nimage = \"{}\"\nfit = \"stretch\"\n",
        written(BLUE),
        picture.display()
    )
}

/// Waits until every output is all `color`, but for a strip of 2 pixels at
/// its edges, where a scaled picture's edge may blend with the colour.
fn wait_all(color: Rgba) {
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |image| all_within(image, color));
    }
}

/// As [`wait_all`], judged on each output once it is still: that the
/// picture stays `color` ([`stays_output`]).
fn stays_all(color: Rgba) {
    for name in &Screen::connect().unwrap().outputs() {
        stays_output(name, |image| all_within(image, color));
    }
}

/// What is not `color` in `image`, but for a strip of 2 pixels at its
/// edges.
fn all_within(image: &Image, color: Rgba) -> Option<String> {
    crate::support::first_other_within(image, color, 2)
        .map(|at| format!("not all {color:?}: {at:?}"))
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_replaced_whole_or_rewritten_in_place_is_shown() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("a.png");
    fs::write(&picture, solid(64, 64, ORANGE)).unwrap();
    write_settings(&stretched(&picture));
    let _wallpaper = Wallpaper::start();
    wait_all(ORANGE);
    // Renamed into place, as an editor or a download saves.
    let staged = dir.path().join("a.png.new");
    fs::write(&staged, solid(64, 64, GREEN)).unwrap();
    fs::rename(&staged, &picture).unwrap();
    wait_all(GREEN);
    // Written again through one open file.
    fs::write(&picture, solid(32, 32, ORANGE)).unwrap();
    wait_all(ORANGE);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_replaced_by_one_that_cannot_be_shown_stays_and_is_said() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("a.png");
    fs::write(&picture, solid(64, 64, ORANGE)).unwrap();
    write_settings(&stretched(&picture));
    let wallpaper = Wallpaper::start();
    wait_all(ORANGE);
    fs::write(&picture, b"not a picture").unwrap();
    wallpaper.wait_said(&format!(
        "cannot show {} as it is now: not a format this decodes",
        picture.display()
    ));
    stays_all(ORANGE);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_taken_away_leaves_the_colour() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("a.png");
    fs::write(&picture, solid(64, 64, ORANGE)).unwrap();
    write_settings(&stretched(&picture));
    let wallpaper = Wallpaper::start();
    wait_all(ORANGE);
    fs::remove_file(&picture).unwrap();
    wallpaper.wait_said(&format!("{} is gone", picture.display()));
    wait_covered(BLUE);
}

/// A PNG's signature and its `IHDR`, claiming `width` × `height` of 8-bit
/// RGBA, and nothing else: refused from the header, before any data.
fn header_only(width: u32, height: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&width.to_be_bytes());
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut chunk = b"IHDR".to_vec();
    chunk.extend_from_slice(&data);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&13_u32.to_be_bytes());
    png.extend_from_slice(&chunk);
    png.extend_from_slice(&crc32(&chunk).to_be_bytes());
    png
}

/// CRC-32 (ISO-HDLC), as PNG's chunks carry it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_over_the_limits_leaves_the_colour_and_says_which() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    let wide = dir.path().join("wide.png");
    fs::write(&wide, header_only(20_000, 1)).unwrap();
    let huge = dir.path().join("huge.png");
    fs::write(&huge, header_only(16_384, 9_000)).unwrap();
    let long = dir.path().join("long.png");
    let file = fs::File::create(&long).unwrap();
    // Sparse: 65 MiB that take no disk.
    file.set_len(65 << 20).unwrap();
    drop(file);
    write_settings(&stretched(&wide));
    let wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    wallpaper.wait_said("it is 20000×1 pixels; the wallpaper shows pictures up to 16384 a side");
    write_settings(&stretched(&huge));
    wallpaper.wait_said("it is 16384×9000 pixels, more than 536870912 bytes decoded");
    write_settings(&stretched(&long));
    wallpaper.wait_said(&format!(
        "the file is {} bytes; the wallpaper reads files of up to {}",
        65_u64 << 20,
        64_u64 << 20
    ));
    stays_covered(BLUE);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_stored_turned_is_shown_upright() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    // Stored with its left half orange and its right half green, and an
    // EXIF orientation of 6: shown turned a quarter clockwise, the orange
    // on top.
    let (w, h) = (64_u32, 32_u32);
    let pixels: Vec<u8> = (0..h)
        .flat_map(|_| (0..w).map(|x| if x < w / 2 { ORANGE } else { GREEN }))
        .flat_map(|c| [c.r, c.g, c.b, 255])
        .collect();
    let stored = encode(w, h, ColorType::Rgba, BitDepth::Eight, &pixels);
    let picture = dir.path().join("turned.png");
    fs::write(
        &picture,
        with_chunk(&stored, *b"eXIf", &jpeg::exif_orientation(6)),
    )
    .unwrap();
    write_settings(&stretched(&picture));
    let _wallpaper = Wallpaper::start();
    for name in &Screen::connect().unwrap().outputs() {
        wait_output(name, |image: &Image| {
            let (iw, ih) = (image.width(), image.height());
            let top = image.pixel(iw / 2, ih / 4).unwrap();
            let bottom = image.pixel(iw / 2, ih * 3 / 4).unwrap();
            (!(about(top, ORANGE) && about(bottom, GREEN)))
                .then(|| format!("top {top:?}, bottom {bottom:?}"))
        });
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_picture_with_a_colour_profile_is_shown_converted() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    // Adobe RGB (220, 30, 30) is sRGB (255, 24, 24), Little CMS 2's figure
    // (`deskrinch-picture`'s colour tests); untagged it would be shown as
    // (220, 30, 30).
    let pixels: Vec<u8> = (0..16 * 16).flat_map(|_| [220, 30, 30]).collect();
    let picture = dir.path().join("adobe.png");
    fs::write(
        &picture,
        with_profile(
            16,
            16,
            ColorType::Rgb,
            BitDepth::Eight,
            &pixels,
            &icc::adobe_rgb(),
        ),
    )
    .unwrap();
    write_settings(&stretched(&picture));
    let _wallpaper = Wallpaper::start();
    wait_all(Rgba::opaque(255, 24, 24));
}
