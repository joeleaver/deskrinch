//! A decoded picture fitted to a target size, as the wallpaper's settings
//! fit one to an output: [`Fit::Cover`], [`Fit::Contain`],
//! [`Fit::Stretch`], [`Fit::Centre`], [`Fit::Tile`].
//!
//! # Resampling
//!
//! Through `fast_image_resize` ([`resample`] says how, and how its working
//! memory follows the target rather than the source):
//!
//! - **Smaller on both axes: in linear light, with a box filter.** The box
//!   filter is the definition, not a finding: each target pixel is the
//!   average of the light of the source it covers, with no ringing.
//! - **Larger on either axis: on sRGB values, with Catmull-Rom.** What was
//!   measured is what settles the rest: averaging light against averaging
//!   sRGB values (sRGB values darken thin lines on a light ground), and,
//!   for a picture made larger, sRGB values against light (sRGB values come
//!   about 1 dB closer, on a photograph and on line art) and the filter
//!   (Catmull-Rom within 0.6 dB of Lanczos3 at about half its time, with
//!   less ringing; it overshoots at an edge, about 11% of a step, clamped to
//!   0 and 255). The
//!   figures are in the pull request that brought this module.
//!
//! # Limits
//!
//! The target is held to the caller's [`Limits`]: its sides to `max_side`,
//! its bytes to `max_decoded_bytes`, and what fitting allocates (the target,
//! the linear copies, the resampler's buffers) to `max_working_bytes`,
//! checked before anything is allocated. The source is the caller's and is
//! not counted.

use std::sync::Arc;

use deskrinch_untrusted::{Image, held, zeroed_pixels};
use fast_image_resize::CropBox;

use crate::limits::Limits;

mod resample;

use resample::resample;

/// How a picture is fitted to a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fit {
    /// Scaled to fill the target, its aspect kept, the overflow cut off
    /// evenly on both sides.
    Cover,
    /// Scaled to fit inside the target, its aspect kept, centred, the rest
    /// transparent.
    Contain,
    /// Scaled to the target's width and height, its aspect not kept.
    Stretch,
    /// Not scaled: centred, cut off where it is larger than the target,
    /// transparent around it where it is smaller.
    Centre,
    /// Not scaled: repeated from the target's top left.
    Tile,
}

/// Why a picture was not fitted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FitError {
    /// The part asked of [`crop`] is not all within the picture.
    #[error("a part {width}×{height} at ({x}, {y}) is not within the picture")]
    Outside {
        /// Its left edge.
        x: u32,
        /// Its top edge.
        y: u32,
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
    /// The target has a side of 0 or over `max_side`.
    #[error("a target of {width}×{height}: a side is 0 or over {max_side}")]
    Dimensions {
        /// The target's width.
        width: u32,
        /// The target's height.
        height: u32,
        /// The bound on each.
        max_side: u32,
    },
    /// The target, width × height × 4 bytes, is over `max_decoded_bytes`.
    #[error("a target of {width}×{height} is more than {max} bytes")]
    TargetTooLarge {
        /// The target's width.
        width: u32,
        /// The target's height.
        height: u32,
        /// The bound.
        max: usize,
    },
    /// Fitting would allocate more than `max_working_bytes` at its peak
    /// (`None`: more than a `usize` can say).
    #[error("fitting needs {needed:?} bytes at its peak, more than {max}")]
    WorkingTooLarge {
        /// What it would need.
        needed: Option<usize>,
        /// The bound.
        max: usize,
    },
    /// `fast_image_resize` refused the resampling: its account.
    #[error("the resampler refused: {0}")]
    Resample(String),
}

/// `image` fitted to `width` × `height` as `how` says, under `limits`.
///
/// # Errors
///
/// [`FitError`]: the target over the side or size bound, or fitting over
/// the working bound.
pub fn fit(
    image: &Image,
    width: u32,
    height: u32,
    how: Fit,
    limits: &Limits,
) -> Result<Image, FitError> {
    let target = target_bytes(width, height, limits)?;
    let (sw, sh) = (f64::from(image.width()), f64::from(image.height()));
    let whole = CropBox {
        left: 0.0,
        top: 0.0,
        width: sw,
        height: sh,
    };
    let rgba = match how {
        Fit::Stretch => resample(image, whole, (width, height), 0, limits)?,
        Fit::Cover => {
            let scale = (f64::from(width) / sw).max(f64::from(height) / sh);
            let (cw, ch) = (f64::from(width) / scale, f64::from(height) / scale);
            let crop = CropBox {
                left: (sw - cw) / 2.0,
                top: (sh - ch) / 2.0,
                width: cw,
                height: ch,
            };
            resample(image, crop, (width, height), 0, limits)?
        }
        Fit::Contain => {
            let scale = (f64::from(width) / sw).min(f64::from(height) / sh);
            let inner = (scaled(sw, scale, width), scaled(sh, scale, height));
            // The canvas is allocated while the resampled picture is too.
            let picture = resample(image, whole, inner, held(target), limits)?;
            let mut canvas = zeroed_pixels(target);
            let at = (
                i64::from((width - inner.0) / 2),
                i64::from((height - inner.1) / 2),
            );
            copy(
                Arc::make_mut(&mut canvas),
                width,
                height,
                &picture,
                inner,
                at,
            );
            canvas
        }
        Fit::Centre => {
            working(Some(held(target)), limits)?;
            let mut canvas = zeroed_pixels(target);
            let at = (
                (i64::from(width) - i64::from(image.width())) / 2,
                (i64::from(height) - i64::from(image.height())) / 2,
            );
            copy(
                Arc::make_mut(&mut canvas),
                width,
                height,
                image.rgba(),
                (image.width(), image.height()),
                at,
            );
            canvas
        }
        Fit::Tile => {
            working(Some(held(target)), limits)?;
            let mut canvas = zeroed_pixels(target);
            let pixels = Arc::make_mut(&mut canvas);
            let (tw, th) = (image.width(), image.height());
            for y in (0..height).step_by(th as usize) {
                for x in (0..width).step_by(tw as usize) {
                    copy(
                        pixels,
                        width,
                        height,
                        image.rgba(),
                        (tw, th),
                        (i64::from(x), i64::from(y)),
                    );
                }
            }
            canvas
        }
    };
    Image::new(width, height, rgba)
        .ok_or_else(|| FitError::Resample("the result is not the target's size".to_owned()))
}

/// The part of `image` `width` × `height` from (`x`, `y`), its pixels as
/// they are, under `limits` (the part's sides and bytes, and its buffer
/// within the working bound): what the wallpaper hands an output that
/// shows only part of a picture at its own size.
///
/// # Errors
///
/// [`FitError::Outside`] for a part not all within the picture; otherwise
/// as [`fit`] for its bounds.
pub fn crop(
    image: &Image,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    limits: &Limits,
) -> Result<Image, FitError> {
    let within = |at: u32, length: u32, whole: u32| {
        length >= 1 && at.checked_add(length).is_some_and(|end| end <= whole)
    };
    if !within(x, width, image.width()) || !within(y, height, image.height()) {
        return Err(FitError::Outside {
            x,
            y,
            width,
            height,
        });
    }
    let target = target_bytes(width, height, limits)?;
    working(Some(held(target)), limits)?;
    let (sw, row) = (image.width() as usize, width as usize * 4);
    Image::filled(width, height, |part| {
        for (r, out) in part.chunks_exact_mut(row).enumerate() {
            let from = ((y as usize + r) * sw + x as usize) * 4;
            out.copy_from_slice(&image.rgba()[from..from + row]);
        }
    })
    .ok_or_else(|| FitError::Resample("the part is not its size".to_owned()))
}

/// The target's bytes, if `width` × `height` is within `limits`.
fn target_bytes(width: u32, height: u32, limits: &Limits) -> Result<usize, FitError> {
    if !(1..=limits.max_side).contains(&width) || !(1..=limits.max_side).contains(&height) {
        return Err(FitError::Dimensions {
            width,
            height,
            max_side: limits.max_side,
        });
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|bytes| *bytes <= limits.max_decoded_bytes)
        .ok_or(FitError::TargetTooLarge {
            width,
            height,
            max: limits.max_decoded_bytes,
        })
}

/// `bytes` if within the working bound.
pub(crate) fn working(bytes: Option<usize>, limits: &Limits) -> Result<usize, FitError> {
    bytes
        .filter(|bytes| *bytes <= limits.max_working_bytes)
        .ok_or(FitError::WorkingTooLarge {
            needed: bytes,
            max: limits.max_working_bytes,
        })
}

/// A side of `side` pixels scaled by `scale`, at least 1 and at most `max`.
fn scaled(side: f64, scale: f64, max: u32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive side no larger than the target's, rounded"
    )]
    let pixels = (side * scale).round() as u32;
    pixels.clamp(1, max)
}

/// Copies `pixels`, `size`, onto `canvas`, `width` × `height`, with its top
/// left at `at` (which may be off the canvas), clipped to the canvas.
fn copy(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    pixels: &[u8],
    size: (u32, u32),
    at: (i64, i64),
) {
    let (w, h) = (i64::from(width), i64::from(height));
    let (pw, ph) = (i64::from(size.0), i64::from(size.1));
    let x0 = at.0.max(0);
    let x1 = (at.0 + pw).min(w);
    if x0 >= x1 {
        return;
    }
    for y in at.1.max(0)..(at.1 + ph).min(h) {
        let (sy, sx) = (y - at.1, x0 - at.0);
        let from = usize::try_from((sy * pw + sx) * 4).unwrap_or(0);
        let to = usize::try_from((y * w + x0) * 4).unwrap_or(0);
        let len = usize::try_from((x1 - x0) * 4).unwrap_or(0);
        canvas[to..to + len].copy_from_slice(&pixels[from..from + len]);
    }
}

#[cfg(test)]
#[path = "fit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "fit_reference_tests.rs"]
mod reference_tests;
