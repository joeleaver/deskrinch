//! A decoded picture fitted to a target size, as the wallpaper's settings
//! fit one to an output: [`Fit::Cover`], [`Fit::Contain`],
//! [`Fit::Stretch`], [`Fit::Centre`], [`Fit::Tile`].
//!
//! # Resampling
//!
//! Through `fast_image_resize`, with alpha premultiplied while resampling:
//!
//! - **Smaller (fewer target pixels than source pixels): in linear light,
//!   with a box filter.** Each target pixel is the average of the light of
//!   the source pixels it covers, which is what an eye sees of the picture
//!   shown smaller; averaging sRGB values instead darkens fine detail, thin
//!   lines on a light ground most of all.
//! - **Larger: on sRGB values, with Catmull-Rom.** Interpolating sRGB
//!   values comes closer to the picture than interpolating light (by about
//!   1 dB, on a photograph and on line art alike), and Catmull-Rom comes
//!   within 0.6 dB of Lanczos3 at about half its time, with less ringing.
//!
//! The measurements behind both choices are in the pull request that
//! brought this module.
//!
//! # Limits
//!
//! The target is held to the caller's [`Limits`]: its sides to `max_side`,
//! its bytes to `max_decoded_bytes`, and what fitting allocates (the target,
//! the linear copies, the resampler's buffers) to `max_working_bytes`,
//! checked before anything is allocated. The source is the caller's and is
//! not counted.

use deskrinch_untrusted::Image;
use fast_image_resize::images::{Image as Pixels, ImageRef};
use fast_image_resize::{
    CropBox, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, SrcCropping,
    create_srgb_mapper,
};

use crate::limits::Limits;

/// What the resampler allocates besides the pictures: its coefficient
/// tables (a few weights for each target column and row) and scratch. Held
/// by the working-bound test.
const RESAMPLER_BYTES: usize = 1024 * 1024;

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
            let picture = resample(image, whole, inner, target, limits)?;
            let mut canvas = vec![0; target];
            let at = (
                i64::from((width - inner.0) / 2),
                i64::from((height - inner.1) / 2),
            );
            copy(&mut canvas, width, height, &picture, inner, at);
            canvas
        }
        Fit::Centre => {
            working(Some(target), limits)?;
            let mut canvas = vec![0; target];
            let at = (
                (i64::from(width) - i64::from(image.width())) / 2,
                (i64::from(height) - i64::from(image.height())) / 2,
            );
            copy(
                &mut canvas,
                width,
                height,
                image.rgba(),
                (image.width(), image.height()),
                at,
            );
            canvas
        }
        Fit::Tile => {
            working(Some(target), limits)?;
            let mut canvas = vec![0; target];
            let (tw, th) = (image.width(), image.height());
            for y in (0..height).step_by(th as usize) {
                for x in (0..width).step_by(tw as usize) {
                    copy(
                        &mut canvas,
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
fn working(bytes: Option<usize>, limits: &Limits) -> Result<usize, FitError> {
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

/// The part `crop` of `image` resampled to `size`: smaller in linear light
/// with a box filter, larger on sRGB values with Catmull-Rom (see the
/// module's documentation). `beside` bytes are held by the caller at the
/// same time and counted.
fn resample(
    image: &Image,
    crop: CropBox,
    size: (u32, u32),
    beside: usize,
    limits: &Limits,
) -> Result<Vec<u8>, FitError> {
    let source = image.width() as usize * image.height() as usize;
    let out = size.0 as usize * size.1 as usize;
    // Smaller when the target has fewer pixels than the part it shows.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a comparison of areas; a few units off decides nothing"
    )]
    let smaller = (out as f64) < crop.width * crop.height;
    // Bytes a pixel of the source and target in the resampling (16-bit
    // linear, or 8-bit sRGB), and what the resampler holds besides: a
    // premultiplied copy of the source and a pass of the target's width
    // over the source's height; and its coefficient tables, at most
    // [`RESAMPLER_BYTES`].
    let depth = if smaller { 8 } else { 4 };
    let pass = size.0 as usize * image.height() as usize;
    let needed = source
        .checked_mul(depth * if smaller { 2 } else { 1 })
        .and_then(|n| n.checked_add(pass.checked_mul(depth)?))
        .and_then(|n| n.checked_add(out.checked_mul(depth)?))
        .and_then(|n| n.checked_add(if smaller { out * 4 } else { 0 }))
        .and_then(|n| n.checked_add(beside))
        .and_then(|n| n.checked_add(RESAMPLER_BYTES));
    working(needed, limits)?;

    let source_view = ImageRef::new(image.width(), image.height(), image.rgba(), PixelType::U8x4)
        .map_err(|e| FitError::Resample(e.to_string()))?;
    let mut resizer = Resizer::new();
    if smaller {
        let mapper = create_srgb_mapper();
        let mut linear = Pixels::new(image.width(), image.height(), PixelType::U16x4);
        mapper
            .forward_map(&source_view, &mut linear)
            .map_err(|e| FitError::Resample(e.to_string()))?;
        let mut scaled_linear = Pixels::new(size.0, size.1, PixelType::U16x4);
        resizer
            .resize(&linear, &mut scaled_linear, &options(crop, FilterType::Box))
            .map_err(|e| FitError::Resample(e.to_string()))?;
        drop(linear);
        let mut target = Pixels::new(size.0, size.1, PixelType::U8x4);
        mapper
            .backward_map(&scaled_linear, &mut target)
            .map_err(|e| FitError::Resample(e.to_string()))?;
        Ok(target.into_vec())
    } else {
        let mut target = Pixels::new(size.0, size.1, PixelType::U8x4);
        resizer
            .resize(
                &source_view,
                &mut target,
                &options(crop, FilterType::CatmullRom),
            )
            .map_err(|e| FitError::Resample(e.to_string()))?;
        Ok(target.into_vec())
    }
}

/// The resampler's options: `filter`, over `crop` of the source, alpha
/// premultiplied.
fn options(crop: CropBox, filter: FilterType) -> ResizeOptions {
    ResizeOptions {
        algorithm: ResizeAlg::Convolution(filter),
        cropping: SrcCropping::Crop(crop),
        mul_div_alpha: true,
    }
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
