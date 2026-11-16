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
