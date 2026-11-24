//! The plan never asks for an image the fit or the crop would refuse, over
//! a grid of the edges: what lets the worker make each need with no check
//! of its own before decoding.

use deskrinch_picture::Limits;

use super::*;
use crate::limits::LIMITS;

/// Surfaces' device sizes: a single pixel, odd sizes, the common ones.
const DEVICES: [(u32, u32); 5] = [
    (1, 1),
    (1281, 721),
    (1920, 1080),
    (1921, 1081),
    (3840, 2160),
];

/// The scales each is laid out at.
const SCALES: [f64; 3] = [1.0, 1.5, 2.0];

const FITS: [Fit; 5] = [
    Fit::Cover,
    Fit::Contain,
    Fit::Stretch,
    Fit::Center,
    Fit::Tile,
];

/// A side of a picture against a surface's side `own`: the smallest, the
/// surface's own and one more, and the largest the limits allow.
fn sides(own: u32) -> [u32; 7] {
    [1, 2, 3, own, own + 1, 16_383, 16_384]
}

/// Whether the limits let a picture `picture` in size be decoded at all.
fn allowed(picture: (u32, u32), limits: &Limits) -> bool {
    let bytes = u64::from(picture.0) * u64::from(picture.1) * 4;
    picture.0 <= limits.max_side
        && picture.1 <= limits.max_side
        && bytes <= limits.max_decoded_bytes as u64
}

/// What `need`, planned from a picture `picture` in size for `surface`
/// with `fit`, would be refused for, if anything.
fn refused(picture: (u32, u32), fit: Fit, surface: Surface, need: Need) -> Option<String> {
    let (w, h) = need.size(picture);
    let bytes = u64::from(w) * u64::from(h) * 4;
    let device = matches!(fit, Fit::Cover | Fit::Contain | Fit::Stretch);
    if w == 0 || h == 0 {
        Some("a side of 0".to_owned())
    } else if w > picture.0 || h > picture.1 {
        Some("larger than the picture".to_owned())
    } else if need != Need::Picture && device && (w > surface.device.0 || h > surface.device.1) {
        Some("larger than the surface".to_owned())
    } else if bytes > LIMITS.max_decoded_bytes as u64 || w > LIMITS.max_side || h > LIMITS.max_side
    {
        Some("over the limits".to_owned())
    } else if let Need::Crop { x, y, .. } = need {
        (u64::from(x) + u64::from(w) > u64::from(picture.0)
            || u64::from(y) + u64::from(h) > u64::from(picture.1))
        .then(|| "a crop outside the picture".to_owned())
    } else {
        None
    }
}

#[test]
fn no_planned_need_is_one_the_fit_or_the_crop_would_refuse() {
    let surfaces: Vec<Surface> = DEVICES
        .iter()
        .flat_map(|&device| SCALES.iter().map(move |&scale| at(device, scale)))
        .collect();
    let mut checked = 0_u32;
    for (index, &first) in surfaces.iter().enumerate() {
        // One, two and three outputs, the others the next surfaces along.
        let sets: Vec<Vec<Surface>> = (1..=3)
            .map(|count| {
                (0..count)
                    .map(|k| surfaces[(index + k * 4) % surfaces.len()])
                    .collect()
            })
            .collect();
        for width in sides(first.device.0) {
            for height in sides(first.device.1) {
                let picture = (width, height);
                if !allowed(picture, &LIMITS) {
                    continue;
                }
                for fit in FITS {
                    for set in &sets {
                        let on = outputs(set);
                        for (key, need) in plan(picture, fit, &on) {
                            if let Some(why) = refused(picture, fit, on[&key], need) {
                                panic!("{need:?}: {why}; {picture:?} {fit:?} on {:?}", on[&key]);
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 10_000, "{checked}");
}

/// The four extremes by name: a 16384-pixel line either way on a 1920x1080
/// output, contained and stretched, asks for a side of at least 1.
#[test]
fn a_line_of_a_picture_is_planned_with_no_side_of_0() {
    let surface = at((1920, 1080), 1.0);
    for picture in [(16_384, 1), (1, 16_384)] {
        for fit in [Fit::Contain, Fit::Stretch] {
            let need = need(picture, fit, surface);
            let (w, h) = need.size(picture);
            assert!(w >= 1 && h >= 1, "{need:?} of {picture:?} by {fit:?}");
            assert!(refused(picture, fit, surface, need).is_none(), "{need:?}");
        }
    }
}
