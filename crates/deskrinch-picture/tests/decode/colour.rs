//! Colour profiles: converted, recognised as sRGB, or ignored with a reason;
//! never a reason to refuse a picture.
//!
//! The expected sRGB values are Little CMS 2's (Pillow 12.1's `ImageCms`)
//! for the same profiles (moxcms's own, encoded), perceptual and relative
//! colorimetric alike: an independent implementation.

use deskrinch_picture::{
    Formats, Ignored, Limits, MAX_CURVE_ENTRIES, MAX_PROFILE_BYTES, Profile, decode,
};
use deskrinch_test_pictures::png::{BitDepth, ColorType, encode, with_profile};
use deskrinch_test_pictures::{fixtures, icc, jpeg};

use crate::{decoded, near, pixel, quadrants};

/// The colours of the Adobe RGB picture.
const COLOURS: [[u8; 3]; 5] = [
    [220, 30, 30],
    [30, 200, 40],
    [30, 40, 210],
    [240, 240, 240],
    [128, 128, 128],
];

fn row(colours: &[[u8; 3]]) -> Vec<u8> {
    colours.concat()
}

#[test]
fn an_srgb_profile_changes_nothing() {
    let pixels = row(&COLOURS);
    let plain = decoded(&encode(5, 1, ColorType::Rgb, BitDepth::Eight, &pixels)).unwrap();
    let tagged = decoded(&with_profile(
        5,
        1,
        ColorType::Rgb,
        BitDepth::Eight,
        &pixels,
        &icc::srgb(),
    ))
    .unwrap();
    assert_eq!(tagged.profile, Profile::AlreadySrgb);
    assert_eq!(tagged.image, plain.image);
    let jpeg = decoded(&jpeg::with_profile(fixtures::BASELINE_JPEG, &icc::srgb())).unwrap();
    assert_eq!(jpeg.profile, Profile::AlreadySrgb);
    assert_eq!(jpeg.image, decoded(fixtures::BASELINE_JPEG).unwrap().image);
}

#[test]
fn an_adobe_rgb_png_is_converted_to_srgb() {
    let pixels = row(&COLOURS);
    let bytes = with_profile(
        5,
        1,
        ColorType::Rgb,
        BitDepth::Eight,
        &pixels,
        &icc::adobe_rgb(),
    );
    let decoded = decoded(&bytes).unwrap();
    assert_eq!(decoded.profile, Profile::Converted);
    let want = [
        [255, 24, 24],
        [0, 201, 0],
        [17, 35, 215],
        [240, 240, 241],
        [129, 129, 129],
    ];
    for (x, [r, g, b]) in (0..).zip(want) {
        let got = pixel(&decoded, x, 0);
        assert!(
            near(got, [r, g, b, 255], 2),
            "{x}: {got:?} for {:?}",
            [r, g, b]
        );
    }
}

#[test]
fn a_display_p3_jpeg_is_converted_to_srgb() {
    // The quadrants as this decoder gives them untagged:
    // (220, 30, 30), (30, 200, 41), (29, 40, 210), (240, 240, 240).
    let decoded = decoded(&jpeg::with_profile(
        fixtures::BASELINE_JPEG,
        &icc::display_p3(),
    ))
    .unwrap();
    assert_eq!(decoded.profile, Profile::Converted);
    let want = [[240, 0, 0], [0, 204, 0], [26, 40, 219], [240, 240, 240]];
    for (got, [r, g, b]) in quadrants(&decoded).into_iter().zip(want) {
        assert!(near(got, [r, g, b, 255], 3), "{got:?} for {:?}", [r, g, b]);
    }
}

#[test]
fn alpha_is_left_as_it_is() {
    let pixels = [220, 30, 30, 77, 30, 200, 40, 0];
    let decoded = decoded(&with_profile(
        2,
        1,
        ColorType::Rgba,
        BitDepth::Eight,
        &pixels,
        &icc::adobe_rgb(),
    ))
    .unwrap();
    assert_eq!(decoded.profile, Profile::Converted);
    assert_eq!(pixel(&decoded, 0, 0)[3], 77);
    assert_eq!(pixel(&decoded, 1, 0)[3], 0);
}

#[test]
fn a_sixteen_bit_png_with_a_profile_is_converted_after_narrowing() {
    let pixels = [220, 0, 30, 0, 30, 0];
    let decoded = decoded(&with_profile(
        1,
        1,
        ColorType::Rgb,
        BitDepth::Sixteen,
        &pixels,
        &icc::adobe_rgb(),
    ))
    .unwrap();
    assert_eq!(decoded.profile, Profile::Converted);
    assert!(near(pixel(&decoded, 0, 0), [255, 24, 24, 255], 2));
}

#[test]
fn a_grey_picture_s_grey_profile_is_ignored() {
    let decoded = decoded(&with_profile(
        2,
        1,
        ColorType::Grayscale,
        BitDepth::Eight,
        &[10, 200],
        &icc::grey(),
    ))
    .unwrap();
    assert_eq!(decoded.profile, Profile::Ignored(Ignored::NotRgb));
    assert_eq!(pixel(&decoded, 1, 0), [200, 200, 200, 255]);
}

#[test]
fn a_bad_profile_is_ignored_and_the_picture_shown() {
    let pixels = row(&COLOURS);
    let plain = decoded(&encode(5, 1, ColorType::Rgb, BitDepth::Eight, &pixels)).unwrap();
    // Not a profile at all.
    let junk = decoded(&with_profile(
        5,
        1,
        ColorType::Rgb,
        BitDepth::Eight,
        &pixels,
        &[7; 300],
    ))
    .unwrap();
    assert!(matches!(
        junk.profile,
        Profile::Ignored(Ignored::Malformed { .. })
    ));
    assert_eq!(junk.image, plain.image);
    // A table over the grid cap (34 points a channel).
    let over = decoded(&jpeg::with_profile(
        fixtures::BASELINE_JPEG,
        &icc::lut(icc::Space::Rgb, 34),
    ))
    .unwrap();
    assert!(
        matches!(over.profile, Profile::Ignored(Ignored::Malformed { .. })),
        "{:?}",
        over.profile
    );
    // Over the size cap.
    let mut large = icc::display_p3();
    large.resize(MAX_PROFILE_BYTES + 1, 0);
    let large = decoded(&jpeg::with_profile(fixtures::BASELINE_JPEG, &large)).unwrap();
    assert!(
        matches!(large.profile, Profile::Ignored(Ignored::TooLarge { .. })),
        "{:?}",
        large.profile
    );
}

#[test]
fn the_largest_table_the_caps_allow_is_read() {
    let profile = icc::lut(icc::Space::Rgb, 33);
    assert!(profile.len() <= MAX_PROFILE_BYTES);
    let decoded = decoded(&jpeg::with_profile(fixtures::BASELINE_JPEG, &profile)).unwrap();
    assert!(
        matches!(decoded.profile, Profile::Converted | Profile::AlreadySrgb),
        "{:?}",
        decoded.profile
    );
}

#[test]
fn converting_over_the_working_bound_ignores_the_profile() {
    let bytes = jpeg::with_profile(fixtures::BASELINE_JPEG, &icc::display_p3());
    let tight = Limits {
        max_encoded_bytes: 1 << 20,
        max_side: 64,
        max_decoded_bytes: 64 * 64 * 4,
        max_working_bytes: 1 << 20,
    };
    let decoded = decode(&bytes, Formats::all(), &tight).unwrap();
    assert_eq!(decoded.profile, Profile::Ignored(Ignored::OverWorkingBound));
}

#[test]
fn the_curve_cap_is_honoured() {
    let at_cap = decoded(&jpeg::with_profile(
        fixtures::BASELINE_JPEG,
        &icc::with_curves(u32::try_from(MAX_CURVE_ENTRIES).unwrap()),
    ))
    .unwrap();
    assert_eq!(at_cap.profile, Profile::Converted);
    let over = decoded(&jpeg::with_profile(
        fixtures::BASELINE_JPEG,
        &icc::with_curves(u32::try_from(MAX_CURVE_ENTRIES + 1).unwrap()),
    ))
    .unwrap();
    assert!(
        matches!(over.profile, Profile::Ignored(Ignored::Malformed { .. })),
        "{:?}",
        over.profile
    );
}

/// A PNG's profile is compressed: one that inflates past what the decoder
/// may allocate for it is dropped by the decoder, and said to be.
#[test]
fn a_png_profile_that_inflates_past_the_decoder_s_limit_is_ignored() {
    let pixels = row(&COLOURS);
    let huge = vec![0; 4 << 20];
    let bytes = with_profile(5, 1, ColorType::Rgb, BitDepth::Eight, &pixels, &huge);
    assert!(bytes.len() < 64 * 1024, "{} bytes", bytes.len());
    let decoded = decoded(&bytes).unwrap();
    assert!(
        matches!(decoded.profile, Profile::Ignored(Ignored::Malformed { .. })),
        "{:?}",
        decoded.profile
    );
    assert_eq!(
        decoded.image,
        crate::decoded(&encode(5, 1, ColorType::Rgb, BitDepth::Eight, &pixels))
            .unwrap()
            .image
    );
}
