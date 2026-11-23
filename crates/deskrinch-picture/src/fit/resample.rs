//! Resampling a part of a picture to a size, with working memory that
//! follows the target, not the source.
//!
//! - **Smaller on both axes: linear light, a box filter, in bands.** The
//!   box filter is the definition of the result, not a finding: each target
//!   pixel is the average of the light of the source it covers, with no
//!   ringing. A target row depends only on the source rows it covers, so
//!   the source is made linear and resampled a band of target rows at a
//!   time, the band's own source rows the crop: the 16-bit copy is a band,
//!   and the result is the same as resampling the whole at once (a test
//!   holds them equal).
//! - **Larger on an axis: sRGB values, Catmull-Rom,** whose kernel the
//!   resampler widens on an axis that shrinks; a box filter would repeat
//!   rows on an axis that grows.
//! - **Alpha premultiplied only where there is alpha:** an opaque picture
//!   (a photograph) is found so once, and resampled without a premultiplied
//!   copy.
//! - **One axis at a time.** `fast_image_resize` 6.1.0, given both axes at
//!   once, shifts the second pass's bounds by the first output pixel's
//!   first source pixel (`resizer.rs:433–436`, `464–467`), which is not the
//!   least of them when that pixel's first weight is exactly zero and is
//!   skipped (`convolution/mod.rs:147–149`): a 4 × 1 picture covering a
//!   27 × 26 target underflows there. A pass that changes one axis only
//!   uses the bounds as they are, so each axis is a pass of its own, with
//!   the premultiplication done once around both.

use std::sync::Arc;

use deskrinch_untrusted::{Image, SHARED_HEADER_BYTES, zeroed_pixels};
use fast_image_resize::images::{Image as Pixels, ImageRef, TypedImage, TypedImageRef};
use fast_image_resize::pixels::{U8x4, U16x4};
use fast_image_resize::{
    CropBox, FilterType, ImageView, ImageViewMut, MulDiv, PixelTrait, PixelType, ResizeAlg,
    ResizeOptions, Resizer, SrcCropping, create_srgb_mapper,
};

use super::{FitError, working};
use crate::limits::Limits;

/// What the resampler allocates besides the pictures: its coefficient
/// tables (a few weights for each target column and row) and scratch. Held
/// by the working-bound test.
const RESAMPLER_BYTES: usize = 1024 * 1024;

/// About how many source rows a band covers.
const BAND_SOURCE_ROWS: f64 = 64.0;

/// The resampler's refusal as this crate's error.
fn refused(error: impl std::fmt::Display) -> FitError {
    FitError::Resample(error.to_string())
}

/// The part `crop` of `image` resampled to `size`, as the module's
/// documentation says. `beside` bytes are held by the caller at the same
/// time and counted.
pub(super) fn resample(
    image: &Image,
    crop: CropBox,
    size: (u32, u32),
    beside: usize,
    limits: &Limits,
) -> Result<Arc<[u8]>, FitError> {
    let opaque = image
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p[3] == u8::MAX);
    let smaller = f64::from(size.0) <= crop.width && f64::from(size.1) <= crop.height;
    if smaller {
        let rows = band_rows(crop.height, size.1);
        smaller_in_bands(image, crop, size, rows, opaque, beside, limits)
    } else {
        larger(image, crop, size, opaque, beside, limits)
    }
}

/// Target rows a band holds when `source_height` source rows make
/// `target_height` target rows: about [`BAND_SOURCE_ROWS`] source rows'
/// worth, at least one.
fn band_rows(source_height: f64, target_height: u32) -> u32 {
    let per_row = source_height / f64::from(target_height);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive count of rows, at most the target's"
    )]
    let rows = (BAND_SOURCE_ROWS / per_row).floor() as u32;
    rows.clamp(1, target_height)
}

/// The smaller path, `rows` target rows a band.
pub(super) fn smaller_in_bands(
    image: &Image,
    crop: CropBox,
    size: (u32, u32),
    rows: u32,
    opaque: bool,
    beside: usize,
    limits: &Limits,
) -> Result<Arc<[u8]>, FitError> {
    let (dw, dh) = (size.0 as usize, size.1 as usize);
    let sw = image.width() as usize;
    let per_row = crop.height / f64::from(size.1);
    // The most source rows a band reads: its share, and a row cut at each
    // end.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a positive count of rows"
    )]
    let band_source =
        ((f64::from(rows) * per_row).ceil() as usize + 2).min(image.height() as usize);
    // The target; the band made linear (premultiplied in place); its
    // horizontal pass; its rows of the target, linear.
    let needed = dw
        .checked_mul(dh)
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(SHARED_HEADER_BYTES))
        .and_then(|n| n.checked_add(beside))
        .and_then(|n| n.checked_add(sw.checked_mul(band_source)?.checked_mul(8)?))
        .and_then(|n| n.checked_add(dw.checked_mul(band_source)?.checked_mul(8)?))
        .and_then(|n| n.checked_add(dw.checked_mul(rows as usize)?.checked_mul(8)?))
        .and_then(|n| n.checked_add(RESAMPLER_BYTES));
    working(needed, limits)?;

    let mapper = create_srgb_mapper();
    let mut resizer = Resizer::new();
    // The result's own shared buffer, written in place.
    let mut result = zeroed_pixels(dw * dh * 4);
    let target = Arc::make_mut(&mut result);
    let source_height = f64::from(image.height());
    let mut r0 = 0;
    while r0 < size.1 {
        let r1 = (r0 + rows).min(size.1);
        let top = crop.top + f64::from(r0) * per_row;
        let bottom = (crop.top + f64::from(r1) * per_row).min(source_height);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "rows within the source"
        )]
        let b0 = (top.floor() as u32).min(image.height() - 1);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "rows within the source"
        )]
        let b1 = (bottom.ceil() as u32).clamp(b0 + 1, image.height());
        let band = &image.rgba()[b0 as usize * sw * 4..b1 as usize * sw * 4];
        let band = ImageRef::new(image.width(), b1 - b0, band, PixelType::U8x4).map_err(refused)?;
        let mut linear = Pixels::new(image.width(), b1 - b0, PixelType::U16x4);
        mapper.forward_map(&band, &mut linear).map_err(refused)?;
        if !opaque {
            MulDiv::default()
                .multiply_alpha_inplace_typed(&mut typed_mut::<U16x4>(&mut linear)?)
                .map_err(refused)?;
        }
        let band_top = top - f64::from(b0);
        let band_crop = CropBox {
            left: crop.left,
            top: band_top,
            width: crop.width,
            height: (bottom - top).min(f64::from(b1 - b0) - band_top),
        };
        let mut out = Pixels::new(size.0, r1 - r0, PixelType::U16x4);
        {
            let mut typed_out = typed_mut::<U16x4>(&mut out)?;
            two_passes(
                &mut resizer,
                &typed::<U16x4>(&linear)?,
                band_crop,
                &mut typed_out,
                FilterType::Box,
            )?;
            if !opaque {
                MulDiv::default()
                    .divide_alpha_inplace_typed(&mut typed_out)
                    .map_err(refused)?;
            }
        }
        drop(linear);
        let rows_out = &mut target[r0 as usize * dw * 4..r1 as usize * dw * 4];
        let mut rows_out =
            Pixels::from_slice_u8(size.0, r1 - r0, rows_out, PixelType::U8x4).map_err(refused)?;
        mapper.backward_map(&out, &mut rows_out).map_err(refused)?;
        r0 = r1;
    }
    Ok(result)
}

/// The larger path.
fn larger(
    image: &Image,
    crop: CropBox,
    size: (u32, u32),
    opaque: bool,
    beside: usize,
    limits: &Limits,
) -> Result<Arc<[u8]>, FitError> {
    let (dw, dh) = (size.0 as usize, size.1 as usize);
    let source = image.width() as usize * image.height() as usize;
    // The target; a premultiplied copy of the source where it has alpha;
    // the horizontal pass over the source's rows.
    let needed = dw
        .checked_mul(dh)
        .and_then(|n| n.checked_mul(4))
        .and_then(|n| n.checked_add(SHARED_HEADER_BYTES))
        .and_then(|n| n.checked_add(beside))
        .and_then(|n| n.checked_add(if opaque { 0 } else { source.checked_mul(4)? }))
        .and_then(|n| n.checked_add(dw.checked_mul(image.height() as usize)?.checked_mul(4)?))
        .and_then(|n| n.checked_add(RESAMPLER_BYTES));
    working(needed, limits)?;

    let source = TypedImageRef::<U8x4>::from_buffer(image.width(), image.height(), image.rgba())
        .map_err(refused)?;
    // The result's own shared buffer, written in place.
    let mut result = zeroed_pixels(dw * dh * 4);
    let mut target = TypedImage::<U8x4>::from_buffer(size.0, size.1, Arc::make_mut(&mut result))
        .map_err(refused)?;
    let mut resizer = Resizer::new();
    if opaque {
        two_passes(
            &mut resizer,
            &source,
            crop,
            &mut target,
            FilterType::CatmullRom,
        )?;
    } else {
        let mut premultiplied = TypedImage::<U8x4>::new(image.width(), image.height());
        MulDiv::default()
            .multiply_alpha_typed(&source, &mut premultiplied)
            .map_err(refused)?;
        two_passes(
            &mut resizer,
            &premultiplied,
            crop,
            &mut target,
            FilterType::CatmullRom,
        )?;
        drop(premultiplied);
        MulDiv::default()
            .divide_alpha_inplace_typed(&mut target)
            .map_err(refused)?;
    }
    drop(target);
    Ok(result)
}

/// `image`'s pixels seen as `P`. The smaller path's images are made as `P`
/// for the sRGB mapper, which takes untyped images only; seen as another
/// type, they are refused.
fn typed<'a, P: PixelTrait>(image: &'a Pixels<'_>) -> Result<TypedImageRef<'a, P>, FitError> {
    image
        .typed_image()
        .ok_or_else(|| refused("an image seen as a pixel type it was not made as"))
}

/// [`typed`], to write.
fn typed_mut<'a, P: PixelTrait>(image: &'a mut Pixels<'_>) -> Result<TypedImage<'a, P>, FitError> {
    image
        .typed_image_mut()
        .ok_or_else(|| refused("an image seen as a pixel type it was not made as"))
}

/// `crop` of `source` resampled into `target` with `filter`, one axis at a
/// time (see the module's documentation): the columns over all of the
/// source's rows, then the rows. Alpha is not touched: the caller has
/// premultiplied it. Typed, so that only the pixel types used here are
/// compiled, not the resampler's every one.
fn two_passes<P: PixelTrait>(
    resizer: &mut Resizer,
    source: &impl ImageView<Pixel = P>,
    crop: CropBox,
    target: &mut impl ImageViewMut<Pixel = P>,
    filter: FilterType,
) -> Result<(), FitError> {
    let rows = source.height();
    let mut columns = TypedImage::<P>::new(target.width(), rows);
    let across = CropBox {
        left: crop.left,
        top: 0.0,
        width: crop.width,
        height: f64::from(rows),
    };
    resizer
        .resize_typed(source, &mut columns, &options(across, filter))
        .map_err(refused)?;
    let down = CropBox {
        left: 0.0,
        top: crop.top,
        width: f64::from(target.width()),
        height: crop.height,
    };
    resizer
        .resize_typed(&columns, target, &options(down, filter))
        .map_err(refused)
}

/// The resampler's options: `filter` over `crop` of the source, alpha left
/// to the caller.
fn options(crop: CropBox, filter: FilterType) -> ResizeOptions {
    ResizeOptions {
        algorithm: ResizeAlg::Convolution(filter),
        cropping: SrcCropping::Crop(crop),
        mul_div_alpha: false,
    }
}
