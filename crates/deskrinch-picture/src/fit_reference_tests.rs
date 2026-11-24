//! `fit` against a plain scalar reference written here, in `f64`, from the
//! definitions it implements: a target smaller on both axes is the box
//! average of the light (sRGB made linear, averaged, made sRGB again); a
//! target larger on an axis is Catmull-Rom over sRGB values, its kernel
//! widened by the scale on an axis that shrinks; a picture with alpha is
//! premultiplied before and divided after. The source window is cut to the
//! picture at its edges and the weights renormalised, as
//! `fast_image_resize` does, and Cover, Contain and Stretch place the
//! result as `fit` says.
//!
//! Each channel must be within a tolerance of the reference: the slack is
//! `fast_image_resize`'s, which works in fixed point (8 bits a channel and
//! its own precision for the weights on the larger path, 16 bits for the
//! linear light on the smaller one) where the reference does not round
//! until the end.
//!
//! An opaque picture is compared as its straight colour. A picture with
//! alpha (kept at 64 and above here) is compared as it is seen: its colour
//! times its alpha, and its alpha beside it, each within the tolerance, so
//! that a fault in the alpha alone cannot hide in the product. Its straight
//! colour is not asserted. The larger path premultiplies in 8 bits, and
//! dividing by the alpha on the way back multiplies that rounding by
//! `255 / a`: measured over these cases, the straight colour is up to 6.4
//! levels off at alphas of 63 to 84 (for Cover, the channels' differences
//! round to 0, 1, 2, 3, 4, 5 and 6 in 1739, 2101, 723, 192, 36, 7 and 2 of
//! them), while colour times alpha stays within 1.8 and the alpha within
//! 1.1. A picture with alpha never reaches a screen except composited, which
//! is the product.

use super::*;

/// What `fit` may differ from the reference by, in levels, on the smaller
/// path. Worst seen over these cases: 1 level of opaque colour; with alpha,
/// 0.84 of colour times alpha and 0.75 of alpha.
const SMALLER_TOLERANCE: u8 = 1;

/// The same on the larger path. Worst seen: 1 level of opaque colour; with
/// alpha, 1.78 of colour times alpha and 1.05 of alpha, which set it.
const LARGER_TOLERANCE: u8 = 2;

/// A deterministic picture of noise, `width` × `height`; with `alpha`, its
/// alpha is noise in 64..=255, else opaque.
fn noise(width: u32, height: u32, alpha: bool, seed: u64) -> Image {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        #[expect(clippy::cast_possible_truncation, reason = "the low byte")]
        let byte = state as u8;
        byte
    };
    Image::filled(width, height, |rgba| {
        for px in rgba.as_chunks_mut::<4>().0 {
            px[0] = next();
            px[1] = next();
            px[2] = next();
            px[3] = if alpha { 64 + next() % 192 } else { 255 };
        }
    })
    .unwrap()
}

fn roomy() -> Limits {
    Limits {
        max_encoded_bytes: 0,
        max_side: 4096,
        max_decoded_bytes: 64 << 20,
        max_working_bytes: 256 << 20,
    }
}

fn srgb_to_linear(v: f64) -> f64 {
    if v < 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f64) -> f64 {
    if v < 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn box_filter(x: f64) -> f64 {
    if x > -0.5 && x <= 0.5 { 1.0 } else { 0.0 }
}

fn catmull_rom(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        (1.5 * x - 2.5) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * -0.5
    } else {
        0.0
    }
}

/// For each of `out` pixels over `in0..in1` of an axis `len` long: its
/// source pixels and their weights, normalised (the window cut to the
/// axis).
fn weights(
    len: u32,
    in0: f64,
    in1: f64,
    out: u32,
    filter: fn(f64) -> f64,
    support: f64,
) -> Vec<Vec<(usize, f64)>> {
    let scale = (in1 - in0) / f64::from(out);
    let filter_scale = scale.max(1.0);
    let radius = support * filter_scale;
    (0..out)
        .map(|o| {
            let centre = in0 + (f64::from(o) + 0.5) * scale;
            let lo = (centre - radius).floor().max(0.0);
            let hi = (centre + radius).ceil().min(f64::from(len));
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "pixel indices within the axis"
            )]
            let (lo, hi) = (lo as usize, hi as usize);
            let taps: Vec<(usize, f64)> = (lo..hi)
                .map(|x| {
                    #[expect(clippy::cast_precision_loss, reason = "a small index")]
                    let at = x as f64 + 0.5 - centre;
                    (x, filter(at / filter_scale))
                })
                .collect();
            let sum: f64 = taps.iter().map(|&(_, w)| w).sum();
            taps.into_iter().map(|(x, w)| (x, w / sum)).collect()
        })
        .collect()
}

/// The reference of `resample`: `crop` (left, top, width, height) of
/// `image` resampled to `size`, as straight-alpha RGBA in 0..=1.
fn reference(image: &Image, crop: (f64, f64, f64, f64), size: (u32, u32)) -> Vec<[f64; 4]> {
    let (sw, sh) = (image.width(), image.height());
    let opaque = image.rgba().as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let smaller = f64::from(size.0) <= crop.2 && f64::from(size.1) <= crop.3;
    let (filter, support): (fn(f64) -> f64, f64) = if smaller {
        (box_filter, 0.5)
    } else {
        (catmull_rom, 2.0)
    };
    // The source as the path sees it: linear light or sRGB values,
    // premultiplied where there is alpha.
    let source: Vec<[f64; 4]> = image
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = f64::from(p[3]) / 255.0;
            let c = |v: u8| {
                let v = f64::from(v) / 255.0;
                let v = if smaller { srgb_to_linear(v) } else { v };
                if opaque { v } else { v * a }
            };
            [c(p[0]), c(p[1]), c(p[2]), a]
        })
        .collect();
    let across = weights(sw, crop.0, crop.0 + crop.2, size.0, filter, support);
    let down = weights(sh, crop.1, crop.1 + crop.3, size.1, filter, support);
    // Columns first over every source row, then rows, as `resample` does.
    let mut columns = vec![[0.0; 4]; size.0 as usize * sh as usize];
    for y in 0..sh as usize {
        for (x, taps) in across.iter().enumerate() {
            let mut sum = [0.0; 4];
            for &(sx, w) in taps {
                let p = source[y * sw as usize + sx];
                (0..4).for_each(|c| sum[c] += p[c] * w);
            }
            // The columns are an image of their own between the passes,
            // 8 bits a channel on the larger path: Catmull-Rom's overshoot
            // is clamped there, before the rows are made from it.
            if !smaller {
                for v in &mut sum {
                    *v = v.clamp(0.0, 1.0);
                }
            }
            columns[y * size.0 as usize + x] = sum;
        }
    }
    let mut out = vec![[0.0; 4]; size.0 as usize * size.1 as usize];
    for (y, taps) in down.iter().enumerate() {
        for x in 0..size.0 as usize {
            let mut sum = [0.0; 4];
            for &(sy, w) in taps {
                let p = columns[sy * size.0 as usize + x];
                (0..4).for_each(|c| sum[c] += p[c] * w);
            }
            let a = sum[3].clamp(0.0, 1.0);
            let colour = |v: f64| {
                let v = v.clamp(0.0, 1.0);
                let v = if opaque {
                    v
                } else if a > 0.0 {
                    (v / a).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                if smaller { linear_to_srgb(v) } else { v }
            };
            out[y * size.0 as usize + x] = [colour(sum[0]), colour(sum[1]), colour(sum[2]), a];
        }
    }
    out
}

/// The reference of `fit` for `how` (Cover, Contain or Stretch).
fn reference_fit(image: &Image, width: u32, height: u32, how: Fit) -> Vec<[f64; 4]> {
    let (sw, sh) = (f64::from(image.width()), f64::from(image.height()));
    let (w, h) = (f64::from(width), f64::from(height));
    match how {
        Fit::Stretch => reference(image, (0.0, 0.0, sw, sh), (width, height)),
        Fit::Cover => {
            let scale = (w / sw).max(h / sh);
            let (cw, ch) = (w / scale, h / scale);
            reference(
                image,
                ((sw - cw) / 2.0, (sh - ch) / 2.0, cw, ch),
                (width, height),
            )
        }
        Fit::Contain => {
            let scale = (w / sw).min(h / sh);
            let inner = (scaled(sw, scale, width), scaled(sh, scale, height));
            let picture = reference(image, (0.0, 0.0, sw, sh), inner);
            let at = ((width - inner.0) / 2, (height - inner.1) / 2);
            let mut canvas = vec![[0.0; 4]; width as usize * height as usize];
            for y in 0..inner.1 {
                for x in 0..inner.0 {
                    canvas[((y + at.1) * width + x + at.0) as usize] =
                        picture[(y * inner.0 + x) as usize];
                }
            }
            canvas
        }
        Fit::Centre | Fit::Tile => unreachable!("no resampling"),
    }
}

/// Fits `image` to `width` × `height` as `how` says and compares it with the
/// reference, failing past `tolerance` levels: an opaque picture channel by
/// channel; one with alpha as each colour times alpha, and its alpha (see
/// the module's comment).
fn check(what: &str, image: &Image, width: u32, height: u32, how: Fit, tolerance: u8) {
    let opaque = image.rgba().as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let fitted = fit(image, width, height, how, &roomy()).unwrap();
    let want = reference_fit(image, width, height, how);
    let tolerance = f64::from(tolerance);
    for (i, (got, want)) in fitted
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(&want)
        .enumerate()
    {
        let got = got.map(|v| f64::from(v) / 255.0);
        let alpha_off = (got[3] - want[3]).abs() * 255.0;
        assert!(
            alpha_off <= tolerance,
            "{what}: pixel {i}'s alpha is {alpha_off:.1} levels from the reference (tolerance \
             {tolerance})"
        );
        for c in 0..3 {
            let (seen, wanted, measure) = if opaque {
                // The reference's rounding is the output's: compare levels.
                (
                    (got[c] * 255.0).round(),
                    (want[c] * 255.0).round(),
                    "colour",
                )
            } else {
                (
                    got[c] * got[3] * 255.0,
                    want[c] * want[3] * 255.0,
                    "colour times alpha",
                )
            };
            let off = (seen - wanted).abs();
            assert!(
                off <= tolerance,
                "{what}: pixel {i} channel {c}'s {measure} is {seen:.1} where the reference is \
                 {wanted:.1} (tolerance {tolerance})"
            );
        }
    }
}

/// The smaller path: linear light, the box, in bands of source rows.
#[test]
fn smaller_is_the_box_average_of_the_light() {
    for alpha in [false, true] {
        let what = |case: &str| format!("{case}, alpha {alpha}");
        // Several bands: 300 source rows, about 64 a band.
        let big = noise(300, 300, alpha, 1);
        check(
            &what("banded"),
            &big,
            70,
            45,
            Fit::Stretch,
            SMALLER_TOLERANCE,
        );
        // An odd size, and a whole-number reduction.
        let odd = noise(97, 61, alpha, 2);
        check(&what("odd"), &odd, 31, 19, Fit::Stretch, SMALLER_TOLERANCE);
        check(
            &what("halved"),
            &noise(64, 48, alpha, 3),
            32,
            24,
            Fit::Stretch,
            SMALLER_TOLERANCE,
        );
        // Cover cuts the overflow; Contain leaves the bars clear.
        let wide = noise(120, 50, alpha, 4);
        check(&what("cover"), &wide, 40, 40, Fit::Cover, SMALLER_TOLERANCE);
        check(
            &what("contain"),
            &wide,
            40,
            40,
            Fit::Contain,
            SMALLER_TOLERANCE,
        );
        // A one-pixel side, of the picture and of the target.
        check(
            &what("1 wide"),
            &noise(1, 50, alpha, 5),
            1,
            7,
            Fit::Stretch,
            SMALLER_TOLERANCE,
        );
        check(
            &what("to 1 wide"),
            &noise(60, 30, alpha, 6),
            1,
            10,
            Fit::Stretch,
            SMALLER_TOLERANCE,
        );
    }
}

/// The larger path: Catmull-Rom over sRGB values, widened on an axis that
/// shrinks.
#[test]
fn larger_is_catmull_rom_over_srgb_values() {
    for alpha in [false, true] {
        let what = |case: &str| format!("{case}, alpha {alpha}");
        check(
            &what("enlarged"),
            &noise(7, 5, alpha, 7),
            23,
            17,
            Fit::Stretch,
            LARGER_TOLERANCE,
        );
        // One axis grows, the other shrinks.
        check(
            &what("mixed"),
            &noise(50, 10, alpha, 8),
            20,
            30,
            Fit::Stretch,
            LARGER_TOLERANCE,
        );
        check(
            &what("odd"),
            &noise(9, 13, alpha, 9),
            31,
            19,
            Fit::Stretch,
            LARGER_TOLERANCE,
        );
        let small = noise(12, 5, alpha, 10);
        check(&what("cover"), &small, 40, 40, Fit::Cover, LARGER_TOLERANCE);
        check(
            &what("contain"),
            &small,
            40,
            40,
            Fit::Contain,
            LARGER_TOLERANCE,
        );
        // A one-pixel side.
        check(
            &what("1 high"),
            &noise(40, 1, alpha, 11),
            90,
            3,
            Fit::Stretch,
            LARGER_TOLERANCE,
        );
    }
}
