//! What each output is handed: the fewest pixels that show what today's CSS
//! shows there, and never more, in all, than the picture itself.
//!
//! # The principles
//!
//! 1. The wallpaper decodes its own picture (bounded, colour-converted,
//!    upright), on its worker.
//! 2. Each output is handed the fewest pixels that show the same thing:
//!    never more than the picture has, never more than the output shows.
//!    Made smaller in advance, never larger: an output that shows the
//!    picture at its own size or larger gets the picture itself, scaled and
//!    placed by rinch's CSS ([`Need::Picture`]).
//! 3. Equal needs share one image: a [`Need`] says what the image is, not
//!    which output it is for, and two outputs of one size share it.
//! 4. Opaque stays opaque: no transparent canvas. Contain's bars, and what
//!    Centre and Tile leave uncovered, are the window's colour behind the
//!    image, and today's CSS places and repeats the image as it always did
//!    ([`crate::style`]).
//! 5. What is held in all is never more than the picture itself: if any
//!    output needs the picture, every output takes it; otherwise, when the
//!    made-smaller images together would be no smaller than the picture,
//!    every output takes the picture instead ([`plan`]). One comparison of
//!    byte counts, the same on both renderers. Its cost: an output that
//!    could have had better made-smaller pixels sometimes gets rinch's
//!    scaling at paint, as before this was written. Memory first.
//!
//!    One exception, held by the worker and not by the plan: an output
//!    whose new image cannot be made (its file gone bad while the picture
//!    is kept, or the image refused) keeps the image it shows beside the
//!    ones newly made, which together may then be more than the picture,
//!    until the file changes or the refused need goes.
//!
//! # Each fit
//!
//! A surface is its size in device pixels (rinch's
//! `window_physical_size`) and in logical pixels (its `window_size`, what
//! it is laid out at):
//!
//! - **Cover**: smaller when the scale that covers the surface is below 1;
//!   then the picture covered at exactly the surface's device size, which
//!   rinch draws one pixel per pixel.
//! - **Contain**: smaller when the scale that fits the picture inside is
//!   below 1; then the picture scaled to its inner box, exactly the
//!   surface's length on the axis that fits, and on the other the nearest
//!   length of the surface's parity, so that centring puts it on a whole
//!   pixel.
//! - **Stretch**: each axis made smaller where the surface is shorter, kept
//!   where it is not.
//! - **Centre** and **Tile**: a picture's pixel is a logical pixel, as CSS
//!   draws an `auto`-sized background (at scale 2 each is 2 × 2 device
//!   pixels), so what the output shows is its logical size. Centre crops an
//!   axis the output is shorter on to its middle, of the picture's parity
//!   so the crop centred lands where that part of the whole did; Tile to
//!   its first pixels, from the top left.

use std::collections::{BTreeMap, BTreeSet};

use deskrinch_settings::wallpaper::Fit;

/// An output's surface: device pixels and logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Surface {
    /// Its size in device pixels: the buffer it is drawn into.
    pub(crate) device: (u32, u32),
    /// Its size in logical pixels: the viewport it is laid out in.
    pub(crate) logical: (u32, u32),
}

/// The image an output is handed, by what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Need {
    /// The decoded picture itself.
    Picture,
    /// The picture covering `width` × `height`, cropped evenly.
    Cover {
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
    /// The whole picture scaled to `width` × `height`.
    Scaled {
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
    /// The part of the picture `width` × `height` from (`x`, `y`), as it is.
    Crop {
        /// Its left edge in the picture.
        x: u32,
        /// Its top edge in the picture.
        y: u32,
        /// Its width.
        width: u32,
        /// Its height.
        height: u32,
    },
}

impl Need {
    /// Its width and height, for a picture `picture` in size.
    pub(crate) fn size(self, picture: (u32, u32)) -> (u32, u32) {
        match self {
            Need::Picture => picture,
            Need::Cover { width, height }
            | Need::Scaled { width, height }
            | Need::Crop { width, height, .. } => (width, height),
        }
    }

    /// Its bytes, for a picture `picture` in size.
    pub(crate) fn bytes(self, picture: (u32, u32)) -> u64 {
        let (w, h) = self.size(picture);
        u64::from(w) * u64::from(h) * 4
    }

    /// Its name in rinch's registry, under `generation` (one per picture
    /// decoded from a changed file): what it is, so equal needs share one.
    pub(crate) fn name(self, generation: u64) -> String {
        match self {
            Need::Picture => format!("wallpaper/{generation}/picture"),
            Need::Cover { width, height } => {
                format!("wallpaper/{generation}/cover/{width}x{height}")
            }
            Need::Scaled { width, height } => {
                format!("wallpaper/{generation}/scaled/{width}x{height}")
            }
            Need::Crop {
                x,
                y,
                width,
                height,
            } => format!("wallpaper/{generation}/crop/{x},{y},{width}x{height}"),
        }
    }
}

/// What one output needs on its own, for a picture `picture` in size,
/// fitted as `fit` says, on `surface` (principles 2 and 4).
pub(crate) fn need(picture: (u32, u32), fit: Fit, surface: Surface) -> Need {
    let (pw, ph) = picture;
    let (w, h) = surface.device;
    let (sx, sy) = (f64::from(w) / f64::from(pw), f64::from(h) / f64::from(ph));
    match fit {
        Fit::Cover if sx.max(sy) < 1.0 => Need::Cover {
            width: w,
            height: h,
        },
        Fit::Contain if sx.min(sy) < 1.0 => {
            let (width, height) = if sx <= sy {
                (w, parity_nearest(f64::from(ph) * sx, h, ph))
            } else {
                (parity_nearest(f64::from(pw) * sy, w, pw), h)
            };
            Need::Scaled { width, height }
        }
        Fit::Stretch if w < pw || h < ph => Need::Scaled {
            width: w.min(pw),
            height: h.min(ph),
        },
        Fit::Center | Fit::Tile => {
            let centred = fit == Fit::Center;
            let (x, width) = axis(pw, surface.logical.0, centred);
            let (y, height) = axis(ph, surface.logical.1, centred);
            if (width, height) == picture {
                Need::Picture
            } else {
                Need::Crop {
                    x,
                    y,
                    width,
                    height,
                }
            }
        }
        _ => Need::Picture,
    }
}

/// The part of a picture's axis `length` long an output `shown` logical
/// pixels long shows: its start and length. All of it when the output is
/// not shorter; otherwise, centred, the middle of the picture's parity
/// (so that the part centred lands where it did in the whole), or, from
/// the top left, its first `shown` pixels.
fn axis(length: u32, shown: u32, centred: bool) -> (u32, u32) {
    if shown >= length {
        return (0, length);
    }
    let shown = shown.max(1);
    if !centred {
        return (0, shown);
    }
    let part = if (length - shown).is_multiple_of(2) {
        shown
    } else {
        shown + 1
    };
    ((length - part) / 2, part)
}

/// The length nearest `exact` with the parity of `side`, from the parity's
/// smallest (1 or 2) to `side` and no longer than `most` (the picture's
/// own length: never made larger): centred in `side`, it starts on a whole
/// pixel. Of two as near, the shorter. Where no length of that parity is
/// within `most`, `most` itself.
fn parity_nearest(exact: f64, side: u32, most: u32) -> u32 {
    let smallest = if side.is_multiple_of(2) { 2 } else { 1 };
    // Candidates of that parity: `smallest + 2k`.
    let k = ((exact - f64::from(smallest)) / 2.0).max(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a non-negative count of steps, no more than half a side"
    )]
    let (below, above) = (
        smallest + 2 * (k.floor() as u32),
        smallest + 2 * (k.ceil() as u32),
    );
    let pick = if f64::from(above) - exact < exact - f64::from(below) {
        above
    } else {
        below
    };
    let longest = if most >= side {
        side
    } else {
        most - (side - most) % 2
    };
    if longest < smallest {
        return most;
    }
    pick.clamp(smallest, longest)
}

/// What every output is handed (principles 3 and 5): each output's own
/// need, unless any output needs the picture itself, or the distinct
/// made-smaller needs together would be no smaller than the picture; then
/// every output takes the picture.
pub(crate) fn plan<K: Ord + Copy>(
    picture: (u32, u32),
    fit: Fit,
    surfaces: &BTreeMap<K, Surface>,
) -> BTreeMap<K, Need> {
    let own: BTreeMap<K, Need> = surfaces
        .iter()
        .map(|(&key, &surface)| (key, need(picture, fit, surface)))
        .collect();
    let distinct: BTreeSet<Need> = own.values().copied().collect();
    let made: u64 = distinct.iter().map(|need| need.bytes(picture)).sum();
    if distinct.contains(&Need::Picture) || made >= Need::Picture.bytes(picture) {
        own.keys().map(|&key| (key, Need::Picture)).collect()
    } else {
        own
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
