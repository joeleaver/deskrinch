use super::*;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn limits() -> Limits {
    Limits {
        max_encoded_bytes: 0,
        max_side: 64,
        max_decoded_bytes: 64 * 64 * 4,
        max_working_bytes: 4 << 20,
    }
}

/// A picture of `pixels`, `width` wide.
fn picture(width: u32, pixels: &[[u8; 4]]) -> Image {
    let height = u32::try_from(pixels.len()).unwrap() / width;
    Image::new(width, height, pixels.concat()).unwrap()
}

fn pixels(image: &Image) -> Vec<[u8; 4]> {
    image.rgba().as_chunks::<4>().0.to_vec()
}

fn near(got: [u8; 4], want: [u8; 4]) -> bool {
    got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1)
}

/// Left half red, right half blue, `width` × `height`.
fn halves(width: u32, height: u32) -> Image {
    let row: Vec<[u8; 4]> = (0..width)
        .map(|x| if x < width / 2 { RED } else { BLUE })
        .collect();
    picture(width, &row.repeat(height as usize))
}

#[test]
fn stretch_takes_the_target_s_sides_and_keeps_its_ends() {
    let fitted = fit(&halves(2, 1), 8, 3, Fit::Stretch, &limits()).unwrap();
    assert_eq!((fitted.width(), fitted.height()), (8, 3));
    let p = pixels(&fitted);
    assert!(near(p[0], RED), "{:?}", p[0]);
    assert!(near(p[7], BLUE), "{:?}", p[7]);
}

/// A black and white checkerboard made a quarter the size: the light of
/// its pixels averaged is half, which is sRGB 188, not 128.
#[test]
fn smaller_averages_light_not_srgb_values() {
    let (black, white) = ([0, 0, 0, 255], [255, 255, 255, 255]);
    let board = picture(2, &[black, white, white, black]);
    let fitted = fit(&board, 1, 1, Fit::Stretch, &limits()).unwrap();
    let [r, g, b, a] = pixels(&fitted)[0];
    assert_eq!(a, 255);
    for value in [r, g, b] {
        assert!((187..=189).contains(&value), "{value}");
    }
}

#[test]
fn smaller_keeps_a_flat_colour() {
    let flat = picture(4, &[[30, 140, 220, 255]; 16]);
    let fitted = fit(&flat, 2, 2, Fit::Stretch, &limits()).unwrap();
    for p in pixels(&fitted) {
        assert!(near(p, [30, 140, 220, 255]), "{p:?}");
    }
}

#[test]
fn cover_fills_the_target_and_cuts_the_overflow_evenly() {
    // 4 × 2 into 2 × 2: the middle two columns, red then blue.
    let fitted = fit(&halves(4, 2), 2, 2, Fit::Cover, &limits()).unwrap();
    assert_eq!(pixels(&fitted), [RED, BLUE, RED, BLUE]);
}

#[test]
fn contain_fits_inside_centred_and_leaves_the_rest_clear() {
    // 4 × 2 into 4 × 4: rows 1 and 2, the clear rows above and below.
    let fitted = fit(&halves(4, 2), 4, 4, Fit::Contain, &limits()).unwrap();
    let p = pixels(&fitted);
    assert!(p[..4].iter().all(|q| *q == CLEAR));
    assert!(p[12..].iter().all(|q| *q == CLEAR));
    assert_eq!(&p[4..8], [RED, RED, BLUE, BLUE]);
    assert_eq!(&p[8..12], [RED, RED, BLUE, BLUE]);
}

#[test]
fn centre_places_the_picture_unscaled_and_cuts_or_clears_around_it() {
    let small = picture(2, &[RED, BLUE, BLUE, RED]);
    let fitted = fit(&small, 4, 4, Fit::Centre, &limits()).unwrap();
    let p = pixels(&fitted);
    assert_eq!([p[5], p[6], p[9], p[10]], [RED, BLUE, BLUE, RED]);
    assert!([0, 3, 12, 15].iter().all(|&i| p[i] == CLEAR));
    // Larger than the target: its middle.
    let numbered: Vec<[u8; 4]> = (0..16u8).map(|n| [n, 0, 0, 255]).collect();
    let fitted = fit(&picture(4, &numbered), 2, 2, Fit::Centre, &limits()).unwrap();
    let middle: Vec<u8> = pixels(&fitted).iter().map(|p| p[0]).collect();
    assert_eq!(middle, [5, 6, 9, 10]);
}

#[test]
fn tile_repeats_the_picture_from_the_top_left_and_cuts_the_last() {
    let fitted = fit(&picture(2, &[RED, BLUE]), 5, 2, Fit::Tile, &limits()).unwrap();
    assert_eq!(pixels(&fitted), [RED, BLUE, RED, BLUE, RED].repeat(2));
}

#[test]
fn the_target_is_held_to_the_limits() {
    let image = halves(2, 2);
    assert!(matches!(
        fit(&image, 0, 4, Fit::Stretch, &limits()),
        Err(FitError::Dimensions { width: 0, .. })
    ));
    assert!(matches!(
        fit(&image, 65, 4, Fit::Stretch, &limits()),
        Err(FitError::Dimensions { width: 65, .. })
    ));
    let small = Limits {
        max_decoded_bytes: 15,
        ..limits()
    };
    assert!(matches!(
        fit(&image, 2, 2, Fit::Tile, &small),
        Err(FitError::TargetTooLarge { .. })
    ));
    let tight = Limits {
        max_working_bytes: 16,
        ..limits()
    };
    for how in [Fit::Cover, Fit::Contain, Fit::Stretch] {
        assert!(matches!(
            fit(&image, 8, 8, how, &tight),
            Err(FitError::WorkingTooLarge { .. })
        ));
    }
}

/// A clear pixel next to a coloured one: premultiplied, its colour does not
/// bleed into the coloured one's neighbours as black.
#[test]
fn alpha_is_premultiplied_while_resampling() {
    let image = picture(2, &[CLEAR, RED]);
    let fitted = fit(&image, 4, 1, Fit::Stretch, &limits()).unwrap();
    let p = pixels(&fitted);
    // Where it is partly opaque, its colour is still red, not darkened.
    for q in p.iter().filter(|q| q[3] > 32) {
        assert!(q[0] > 200 && q[1] < 20 && q[2] < 20, "{q:?}");
    }
}

/// Grey `value`, opaque.
fn grey(value: u8) -> [u8; 4] {
    [value, value, value, 255]
}

/// Stretched so that the width shrinks and the height grows (the area
/// smaller): the rows on the growing axis are interpolated, not repeated.
#[test]
fn an_axis_that_grows_is_interpolated_even_when_the_area_shrinks() {
    let gradient = picture(8, &[[grey(0); 8], [grey(200); 8]].concat());
    let fitted = fit(&gradient, 2, 4, Fit::Stretch, &limits()).unwrap();
    let column: Vec<u8> = pixels(&fitted).iter().step_by(2).map(|p| p[0]).collect();
    assert!(
        column[0] < column[1] && column[1] < column[2] && column[2] < column[3],
        "{column:?}"
    );
    assert!(column[1] > 0 && column[2] < 200, "{column:?}");
}

#[test]
fn cover_and_contain_at_a_taller_aspect() {
    // A picture taller than the target: 2 × 4, top half red, bottom blue.
    let tall = picture(2, &[RED, RED, RED, RED, BLUE, BLUE, BLUE, BLUE]);
    // Covering 2 × 2: the middle two rows, red then blue.
    let covered = fit(&tall, 2, 2, Fit::Cover, &limits()).unwrap();
    assert_eq!(pixels(&covered), [RED, RED, BLUE, BLUE]);
    // Contained in 4 × 4: columns 1 and 2, clear columns at either side.
    let contained = fit(&tall, 4, 4, Fit::Contain, &limits()).unwrap();
    for (row, colour) in [(0, RED), (1, RED), (2, BLUE), (3, BLUE)] {
        let p = &pixels(&contained)[row * 4..row * 4 + 4];
        assert_eq!(p, [CLEAR, colour, colour, CLEAR], "row {row}");
    }
}

#[test]
fn larger_keeps_a_flat_colour() {
    let flat = picture(2, &[[30, 140, 220, 255]; 4]);
    let fitted = fit(&flat, 9, 7, Fit::Stretch, &limits()).unwrap();
    for p in pixels(&fitted) {
        assert!(near(p, [30, 140, 220, 255]), "{p:?}");
    }
}

/// A two-pixel step made larger rises through values in between, without a
/// dip. Catmull-Rom overshoots at an edge (its kernel has negative lobes):
/// on a step from 50 to 200 made four times wider it goes 17 levels past
/// each end (33 and 217), about 11% of the step; it is never past 0 or
/// 255, which clamp it.
#[test]
fn larger_rises_through_a_step_and_its_overshoot_is_small_and_clamped() {
    let step = picture(2, &[grey(50), grey(200)]);
    let fitted = fit(&step, 8, 1, Fit::Stretch, &limits()).unwrap();
    let row: Vec<u8> = pixels(&fitted).iter().map(|p| p[0]).collect();
    assert!(row.windows(2).all(|w| w[0] <= w[1]), "{row:?}");
    assert!(row.iter().any(|v| (51..200).contains(v)), "{row:?}");
    // At most 12% of the step past either end.
    assert!(row.iter().all(|v| (32..=218).contains(v)), "{row:?}");
    // A step from 0 to 255 overshoots past both ends: clamped there.
    let hard = picture(2, &[grey(0), grey(255)]);
    let fitted = fit(&hard, 8, 1, Fit::Stretch, &limits()).unwrap();
    let row: Vec<u8> = pixels(&fitted).iter().map(|p| p[0]).collect();
    assert_eq!((row[0], row[7]), (0, 255), "{row:?}");
}

/// Noise, opaque or not.
fn noise(width: u32, height: u32, opaque: bool) -> Image {
    let mut x: u32 = 0x9e37_79b9 ^ width ^ (height << 16);
    let rgba: Vec<u8> = (0..width * height * 4)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if opaque && i % 4 == 3 {
                255
            } else {
                x.to_le_bytes()[0]
            }
        })
        .collect();
    Image::new(width, height, rgba).unwrap()
}

/// Resampling in bands gives what resampling the whole at once gives.
#[test]
fn bands_give_the_whole_s_result() {
    let roomy = Limits {
        max_working_bytes: 64 << 20,
        ..limits()
    };
    for opaque in [true, false] {
        let image = noise(61, 47, opaque);
        let crop = CropBox {
            left: 1.3,
            top: 2.7,
            width: 57.1,
            height: 41.9,
        };
        let size = (13, 11);
        let whole =
            resample::smaller_in_bands(&image, crop, size, size.1, opaque, 0, &roomy).unwrap();
        for rows in [1, 2, 3, 5] {
            let banded =
                resample::smaller_in_bands(&image, crop, size, rows, opaque, 0, &roomy).unwrap();
            assert_eq!(banded, whole, "opaque {opaque}, {rows} rows a band");
        }
    }
}

proptest::proptest! {
    #![proptest_config(deskrinch_untrusted::fuzz::config())]

    /// Every fit of every size from 1 to 64 a side gives the target's size.
    #[test]
    fn every_fit_of_every_size_gives_the_target(
        source in (1_u32..=64, 1_u32..=64),
        target in (1_u32..=64, 1_u32..=64),
        opaque in proptest::bool::ANY,
        how in proptest::sample::select(vec![
            Fit::Cover, Fit::Contain, Fit::Stretch, Fit::Centre, Fit::Tile,
        ]),
    ) {
        let roomy = Limits {
            max_working_bytes: 64 << 20,
            ..limits()
        };
        let image = noise(source.0, source.1, opaque);
        let fitted = fit(&image, target.0, target.1, how, &roomy);
        proptest::prop_assert!(fitted.is_ok(), "{how:?} {source:?} to {target:?}: {fitted:?}");
        let fitted = fitted.unwrap();
        proptest::prop_assert_eq!((fitted.width(), fitted.height()), target);
    }
}

/// What the property test found: a 4 × 1 picture covering 27 × 26, where
/// the resampler, given both axes at once, underflowed.
#[test]
fn a_narrow_crop_made_much_larger_is_resampled() {
    for opaque in [true, false] {
        let fitted = fit(&noise(4, 1, opaque), 27, 26, Fit::Cover, &limits()).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (27, 26));
    }
}

/// A part is its pixels as they are, and a part not within the picture, or
/// over a bound, is refused.
#[test]
fn a_crop_is_the_part_as_it_is() {
    let image = noise(5, 4, false);
    let part = crop(&image, 1, 2, 3, 2, &limits()).unwrap();
    assert_eq!((part.width(), part.height()), (3, 2));
    for r in 0..2 {
        let from = ((2 + r) * 5 + 1) * 4;
        assert_eq!(
            &part.rgba()[r * 12..r * 12 + 12],
            &image.rgba()[from..from + 12]
        );
    }
    let whole = crop(&image, 0, 0, 5, 4, &limits()).unwrap();
    assert_eq!(whole.rgba(), image.rgba());
    for (x, y, w, h) in [
        (3, 0, 3, 1),
        (0, 4, 1, 1),
        (0, 0, 0, 1),
        (u32::MAX, 0, 2, 1),
    ] {
        assert!(
            matches!(
                crop(&image, x, y, w, h, &limits()),
                Err(FitError::Outside { .. })
            ),
            "{x},{y} {w}x{h}"
        );
    }
    let tight = Limits {
        max_working_bytes: 3 * 2 * 4,
        ..limits()
    };
    assert!(matches!(
        crop(&image, 1, 2, 3, 2, &tight),
        Err(FitError::WorkingTooLarge { .. })
    ));
}
