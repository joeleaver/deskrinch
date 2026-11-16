//! WebP: lossless, lossy, alpha, and an animation's first frame.

use deskrinch_picture::Format;
use deskrinch_test_pictures::{QUADRANTS, fixtures, webp};

use crate::{decoded, near, pixel, quadrants};

#[test]
fn a_lossless_webp_comes_back_as_it_went_in() {
    let rgba: Vec<u8> = (0..4 * 3 * 4)
        .map(|i| u8::try_from(i * 5).unwrap())
        .collect();
    let decoded = decoded(&webp::lossless(4, 3, &rgba)).unwrap();
    assert_eq!(decoded.format, Format::WebP);
    assert_eq!(decoded.image.rgba(), rgba.as_slice());
    let rgb: Vec<u8> = (0..4 * 3 * 3)
        .map(|i| u8::try_from(i * 7).unwrap())
        .collect();
    let decoded = crate::decoded(&webp::lossless_rgb(4, 3, &rgb)).unwrap();
    let opaque: Vec<u8> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    assert_eq!(decoded.image.rgba(), opaque.as_slice());
}

#[test]
fn a_lossy_webp_gives_the_quadrants_and_its_alpha() {
    let decoded = decoded(fixtures::LOSSY_WEBP).unwrap();
    for (got, [r, g, b]) in quadrants(&decoded).into_iter().zip(QUADRANTS) {
        assert!(near(got, [r, g, b, 255], 10), "{got:?}");
    }
    let decoded = crate::decoded(fixtures::LOSSY_ALPHA_WEBP).unwrap();
    for (got, [r, g, b]) in quadrants(&decoded).into_iter().zip(QUADRANTS) {
        assert!(near(got, [r, g, b, 128], 10), "{got:?}");
    }
}

#[test]
fn an_animation_gives_its_first_frame_on_its_canvas() {
    let red = [255, 0, 0, 255].repeat(4 * 2);
    let blue = [0, 0, 255, 255].repeat(8 * 6);
    let bytes = webp::animated(8, 6, &[(2, 2, 4, 2, red), (0, 0, 8, 6, blue)]);
    let decoded = decoded(&bytes).unwrap();
    assert_eq!((decoded.image.width(), decoded.image.height()), (8, 6));
    // Blended onto the transparent canvas, the red may round down by one.
    assert!(near(pixel(&decoded, 2, 2), [255, 0, 0, 255], 1));
    assert!(near(pixel(&decoded, 5, 3), [255, 0, 0, 255], 1));
    assert_eq!(pixel(&decoded, 0, 0)[3], 0);
}
