//! JPEG: baseline, progressive, subsampled, grey, CMYK, orientation, scans.

use deskrinch_picture::{DecodeError, Format, Ignored, MAX_SCANS, Profile};
use deskrinch_test_pictures::{QUADRANTS, fixtures, icc, jpeg};

use crate::{decoded, near, pixel, quadrants};

/// The quadrants' colours, opaque.
fn opaque_quadrants() -> [[u8; 4]; 4] {
    QUADRANTS.map(|[r, g, b]| [r, g, b, 255])
}

#[test]
fn baseline_progressive_and_subsampled_give_the_quadrants() {
    for (name, bytes, tolerance) in [
        ("baseline", fixtures::BASELINE_JPEG, 3),
        ("progressive", fixtures::PROGRESSIVE_JPEG, 3),
        // Chroma at half resolution blurs the colours a little more.
        ("4:2:0", fixtures::SUBSAMPLED_JPEG, 12),
    ] {
        let decoded = decoded(bytes).unwrap();
        assert_eq!(decoded.format, Format::Jpeg);
        assert_eq!((decoded.image.width(), decoded.image.height()), (32, 16));
        for (got, want) in quadrants(&decoded).into_iter().zip(opaque_quadrants()) {
            assert!(near(got, want, tolerance), "{name}: {got:?} for {want:?}");
        }
    }
}

#[test]
fn a_grey_jpeg_is_widened() {
    let decoded = decoded(fixtures::GREY_JPEG).unwrap();
    assert!(near(pixel(&decoded, 5, 5), [100, 100, 100, 255], 2));
}

/// zune converts CMYK (Adobe's inverted CMYK, as Photoshop and Pillow write
/// it) to RGB itself; (0, 128, 255, 0) is a strong orange.
#[test]
fn a_cmyk_jpeg_is_converted_by_the_decoder_and_its_print_profile_ignored() {
    let decoded = decoded(fixtures::CMYK_JPEG).unwrap();
    let [r, g, b, a] = pixel(&decoded, 5, 5);
    assert_eq!(a, 255);
    assert!(r > 200 && (100..160).contains(&g) && b < 40, "{r} {g} {b}");
    let with_profile = jpeg::with_profile(fixtures::CMYK_JPEG, &icc::lut(icc::Space::Cmyk, 5));
    let decoded = crate::decoded(&with_profile).unwrap();
    assert_eq!(decoded.profile, Profile::Ignored(Ignored::NotRgb));
    assert_eq!(pixel(&decoded, 5, 5), [r, g, b, a]);
}

/// Each EXIF orientation shows the 32 × 16 quadrants upright: the colour
/// that ends up at each corner of what is shown, and its size.
#[test]
fn each_exif_orientation_is_applied() {
    let [tl, tr, bl, br] = opaque_quadrants();
    let cases = [
        (1, (32, 16), [tl, tr, bl, br]),
        (2, (32, 16), [tr, tl, br, bl]),
        (3, (32, 16), [br, bl, tr, tl]),
        (4, (32, 16), [bl, br, tl, tr]),
        (5, (16, 32), [tl, bl, tr, br]),
        (6, (16, 32), [bl, tl, br, tr]),
        (7, (16, 32), [br, tr, bl, tl]),
        (8, (16, 32), [tr, br, tl, bl]),
    ];
    for (value, size, corners) in cases {
        let decoded = decoded(&jpeg::with_orientation(fixtures::BASELINE_JPEG, value)).unwrap();
        let (w, h) = (decoded.image.width(), decoded.image.height());
        assert_eq!((w, h), size, "orientation {value}");
        let shown = [
            pixel(&decoded, 2, 2),
            pixel(&decoded, w - 3, 2),
            pixel(&decoded, 2, h - 3),
            pixel(&decoded, w - 3, h - 3),
        ];
        for (got, want) in shown.into_iter().zip(corners) {
            assert!(near(got, want, 3), "orientation {value}: {shown:?}");
        }
    }
    // An orientation that is none of the eight is left alone.
    let odd = decoded(&jpeg::with_orientation(fixtures::BASELINE_JPEG, 9)).unwrap();
    assert_eq!((odd.image.width(), odd.image.height()), (32, 16));
}

#[test]
fn more_scans_than_the_cap_are_refused_and_the_cap_itself_is_decoded() {
    // The fixture has 10 scans (libjpeg's default progressive script).
    let at_cap = jpeg::with_repeated_scan(fixtures::PROGRESSIVE_JPEG, MAX_SCANS - 10);
    assert!(decoded(&at_cap).is_ok());
    let over = jpeg::with_repeated_scan(fixtures::PROGRESSIVE_JPEG, MAX_SCANS - 9);
    assert!(matches!(
        decoded(&over),
        Err(DecodeError::Malformed {
            format: Format::Jpeg,
            ..
        })
    ));
}

#[test]
fn a_truncated_jpeg_never_panics() {
    for end in 0..fixtures::PROGRESSIVE_JPEG.len() {
        drop(decoded(&fixtures::PROGRESSIVE_JPEG[..end]));
    }
}
