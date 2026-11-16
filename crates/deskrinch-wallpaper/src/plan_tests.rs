use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;

const K4: (u32, u32) = (3840, 2160);

/// A surface at `scale`, `device` in size, laid out as rinch does: the
/// device size divided by the scale, rounded.
fn at(device: (u32, u32), scale: f64) -> Surface {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive size"
    )]
    let logical = |d: u32| (f64::from(d) / scale).round() as u32;
    Surface {
        device,
        logical: (logical(device.0), logical(device.1)),
    }
}

fn outputs(surfaces: &[Surface]) -> BTreeMap<usize, Surface> {
    surfaces.iter().copied().enumerate().collect()
}

fn held(picture: (u32, u32), plan: &BTreeMap<usize, Need>) -> u64 {
    let distinct: BTreeSet<Need> = plan.values().copied().collect();
    distinct.iter().map(|need| need.bytes(picture)).sum()
}

#[test]
fn cover_smaller_is_the_surfaces_device_size_and_not_smaller_is_the_picture() {
    assert_eq!(
        need(K4, Fit::Cover, at((1280, 720), 1.0)),
        Need::Cover {
            width: 1280,
            height: 720
        }
    );
    // A fractional scale changes nothing: the device size is what is drawn.
    assert_eq!(
        need(K4, Fit::Cover, at((2560, 1440), 1.25)),
        Need::Cover {
            width: 2560,
            height: 1440
        }
    );
    assert_eq!(need(K4, Fit::Cover, at(K4, 1.0)), Need::Picture);
    assert_eq!(
        need((640, 480), Fit::Cover, at((1280, 720), 1.0)),
        Need::Picture
    );
    // Taller than the picture on one axis only: covering it needs more.
    assert_eq!(need(K4, Fit::Cover, at((1080, 2400), 1.0)), Need::Picture);
}

#[test]
fn contain_is_exact_on_the_axis_that_fits_and_of_the_surfaces_parity_on_the_other() {
    // 4:3 into 16:9: the height fits, the width is 1440 (even, as 1920).
    assert_eq!(
        need((4000, 3000), Fit::Contain, at((1920, 1080), 1.0)),
        Need::Scaled {
            width: 1440,
            height: 1080
        }
    );
    // 3840 × 2160 into 1281 × 1000: the width fits, 720.56… high, and the
    // nearest even length (1000 is even) is 720.
    assert_eq!(
        need(K4, Fit::Contain, at((1281, 1000), 1.0)),
        Need::Scaled {
            width: 1281,
            height: 720
        }
    );
    // Of the surface's parity: 1001 is odd, so 721.
    assert_eq!(
        need(K4, Fit::Contain, at((1281, 1001), 1.0)),
        Need::Scaled {
            width: 1281,
            height: 721
        }
    );
    assert_eq!(
        need((800, 600), Fit::Contain, at((1920, 1080), 1.0)),
        Need::Picture
    );
}

#[test]
fn stretch_makes_each_axis_smaller_only_where_the_surface_is_shorter() {
    assert_eq!(
        need(K4, Fit::Stretch, at((1280, 720), 1.0)),
        Need::Scaled {
            width: 1280,
            height: 720
        }
    );
    assert_eq!(
        need((2000, 500), Fit::Stretch, at((1280, 720), 1.0)),
        Need::Scaled {
            width: 1280,
            height: 500
        }
    );
    assert_eq!(
        need((800, 600), Fit::Stretch, at((1280, 720), 1.0)),
        Need::Picture
    );
}

#[test]
fn centre_crops_the_middle_in_logical_pixels_of_the_pictures_parity() {
    // 1920 × 1080 device at scale 2 is 960 × 540 logical: the middle 960 ×
    // 540 of the picture, each pixel drawn 2 × 2 as today.
    assert_eq!(
        need(K4, Fit::Center, at((1920, 1080), 2.0)),
        Need::Crop {
            x: 1440,
            y: 810,
            width: 960,
            height: 540
        }
    );
    // 1281 logical across 3840: one more, 1282, so that (3840 − 1282) / 2
    // is whole and the crop centred lands where its part of the whole did.
    assert_eq!(
        need(K4, Fit::Center, at((1281, 2160), 1.0)),
        Need::Crop {
            x: 1279,
            y: 0,
            width: 1282,
            height: 2160
        }
    );
    assert_eq!(
        need((256, 256), Fit::Center, at((1920, 1080), 1.0)),
        Need::Picture
    );
}

#[test]
fn tile_crops_the_top_left_in_logical_pixels_and_keeps_an_axis_that_repeats() {
    assert_eq!(
        need((2000, 300), Fit::Tile, at((1280, 720), 1.0)),
        Need::Crop {
            x: 0,
            y: 0,
            width: 1280,
            height: 300
        }
    );
    assert_eq!(need((256, 256), Fit::Tile, at(K4, 1.0)), Need::Picture);
}

#[test]
fn equal_needs_are_one_name_and_names_say_what_the_image_is() {
    let plan = plan(
        K4,
        Fit::Cover,
        &outputs(&[at((1280, 720), 1.0), at((1280, 720), 1.0)]),
    );
    let names: BTreeSet<String> = plan.values().map(|need| need.name(3)).collect();
    assert_eq!(
        names,
        BTreeSet::from(["wallpaper/3/cover/1280x720".to_owned()])
    );
    assert_eq!(Need::Picture.name(1), "wallpaper/1/picture");
    assert_eq!(
        Need::Crop {
            x: 1,
            y: 2,
            width: 3,
            height: 4
        }
        .name(9),
        "wallpaper/9/crop/1,2,3x4"
    );
}

/// The note's four cases: never more than the decoded picture, and less
/// where every output is smaller than it.
#[test]
fn the_four_cases_hold_no_more_than_the_picture() {
    let picture = Need::Picture.bytes(K4);
    let two_720 = plan(K4, Fit::Cover, &outputs(&[at((1280, 720), 1.0); 2]));
    assert_eq!(held(K4, &two_720), 1280 * 720 * 4, "one shared 720p image");
    let two_4k = plan(K4, Fit::Cover, &outputs(&[at(K4, 1.0); 2]));
    assert_eq!(held(K4, &two_4k), picture, "the picture, shared");
    let mixed = plan(
        K4,
        Fit::Cover,
        &outputs(&[at((1920, 1080), 1.0), at(K4, 1.0)]),
    );
    assert!(
        mixed.values().all(|need| *need == Need::Picture),
        "{mixed:?}"
    );
    assert_eq!(held(K4, &mixed), picture);
    let tile = plan((256, 256), Fit::Tile, &outputs(&[at(K4, 1.0)]));
    assert_eq!(held((256, 256), &tile), 256 * 256 * 4);
}

/// The fifth principle's own example: 18.9 and 14.1 MiB together are more
/// than the 31.6 of the picture, so both outputs take the picture.
#[test]
fn made_smaller_images_that_together_are_not_smaller_give_way_to_the_picture() {
    let plan = plan(
        K4,
        Fit::Cover,
        &outputs(&[at((3440, 1440), 1.0), at((2560, 1440), 1.0)]),
    );
    assert!(plan.values().all(|need| *need == Need::Picture), "{plan:?}");
}

fn fits() -> impl Strategy<Value = Fit> {
    prop_oneof![
        Just(Fit::Cover),
        Just(Fit::Contain),
        Just(Fit::Stretch),
        Just(Fit::Center),
        Just(Fit::Tile),
    ]
}

fn scales() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(1.0),
        Just(1.25),
        Just(1.5),
        Just(1.75),
        Just(2.0),
        Just(3.0)
    ]
}

proptest! {
    /// Over sizes, fits, scales and sets of outputs: no image is larger than
    /// the picture on either axis, none larger than its surface shows, and
    /// all of them together are never more than the picture's bytes.
    #[test]
    fn never_more_than_the_picture(
        picture in (1_u32..5000, 1_u32..5000),
        fit in fits(),
        surfaces in prop::collection::vec(((1_u32..5000, 1_u32..5000), scales()), 1..5),
    ) {
        let surfaces: Vec<Surface> = surfaces.into_iter().map(|(d, s)| at(d, s)).collect();
        let surfaces = outputs(&surfaces);
        let plan = plan(picture, fit, &surfaces);
        prop_assert_eq!(plan.len(), surfaces.len());
        prop_assert!(held(picture, &plan) <= Need::Picture.bytes(picture));
        for (key, need) in &plan {
            let (w, h) = need.size(picture);
            prop_assert!(w >= 1 && h >= 1);
            prop_assert!(w <= picture.0 && h <= picture.1, "{need:?} of {picture:?}");
            if *need != Need::Picture {
                let surface = surfaces[key];
                let shows = match fit {
                    Fit::Center | Fit::Tile => (surface.logical.0 + 1, surface.logical.1 + 1),
                    _ => surface.device,
                };
                prop_assert!(w <= shows.0 && h <= shows.1, "{need:?} on {surface:?}");
            }
        }
    }
}
