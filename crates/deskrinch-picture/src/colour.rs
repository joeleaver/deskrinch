//! An embedded ICC profile converted to sRGB, on the RGBA8 result, row by
//! row through one row of scratch, with moxcms used directly.
//!
//! What it does with a picture's profile is said beside the picture
//! ([`Profile`]), never logged: a stranger chooses the file, and the log is
//! the caller's. A profile that cannot be used never refuses the picture.
//!
//! - **sRGB already:** most photographs carry an sRGB profile. It is
//!   recognised by what it does: the transform from it to sRGB is built and
//!   run over a grid of 17 levels a channel (4913 colours) and all 256
//!   greys, and if every one comes back unchanged the pixels are left as
//!   they are ([`Profile::AlreadySrgb`]). moxcms has no comparison of its
//!   own, and comparing colorants and curves would miss profiles that
//!   describe sRGB in other words.
//! - **The caps are this crate's**, passed to moxcms's parse in place of
//!   its defaults (10 MB, 10 million grid points, 40 000 curve entries):
//!   [`MAX_PROFILE_BYTES`], [`MAX_GRID_POINTS`], [`MAX_CURVE_ENTRIES`].
//!   Their reasons are on each. They are a property of the decoder, not
//!   desktop policy.
//! - **Not converted:** a profile that is not RGB (a grey picture's grey
//!   profile; a CMYK JPEG's print profile), and PNG's `cICP`, `gAMA` and
//!   `cHRM` chunks, which this crate does not read.

use moxcms::{ColorProfile, DataColorSpace, Layout, ParsingOptions, TransformOptions};

/// The largest profile read, in bytes: 256 KiB. The profiles pictures
/// embed are working spaces (sRGB, Display P3, Adobe RGB, ProPhoto,
/// Rec. 2020): matrix-and-curves profiles. The RGB matrix profiles in
/// `/usr/share/color/icc` on the machine this was written on are 372 bytes
/// to 25 KB; the two RGB profiles there with lookup tables are 60 KB (a
/// 17-point table each way) and 157 KB (seven 17-point tables). One 33-point
/// RGB table of 16-bit entries is 33³ × 3 × 2 = 214 KB. A monitor's
/// calibration profile can be larger, but it describes a display, not a
/// picture; print profiles are larger still, and CMYK, which is not
/// converted.
pub const MAX_PROFILE_BYTES: usize = 256 * 1024;

/// The most points a profile's lookup table may have: 33 a channel, 35 937
/// for RGB. The installed RGB profiles with tables use 17; 33 is what
/// calibration tools write at their high settings.
pub const MAX_GRID_POINTS: usize = 33 * 33 * 33;

/// The most entries a profile's tone curve may have: 4096, what camera and
/// display profiles use at most (1024 and 4096 are usual; matrix profiles
/// mostly carry a gamma or a parametric curve).
pub const MAX_CURVE_ENTRIES: usize = 4096;

/// What converting may allocate besides the result, with the caps above:
/// the profile parsed, the transform's tables, the probe and one row. Held
/// by the working-bound test on the largest profile the caps allow.
pub(crate) const WORKING_BYTES: usize = 4 * 1024 * 1024;

/// What converting a picture `width` wide with a profile of `profile`
/// bytes may allocate besides the result.
pub(crate) fn extra(profile: usize, width: usize) -> usize {
    WORKING_BYTES + profile + width * 4
}

/// What happened to the picture's colour profile. The picture is shown in
/// every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Profile {
    /// It has none: its values are taken as sRGB.
    None,
    /// It has an sRGB profile: the pixels are as decoded.
    AlreadySrgb,
    /// Its RGB profile was converted to sRGB.
    Converted,
    /// Its profile was not used, for this reason; the values are taken as
    /// sRGB.
    Ignored(Ignored),
}

/// Why a picture's profile was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ignored {
    /// It is larger than [`MAX_PROFILE_BYTES`].
    TooLarge {
        /// Its size in bytes.
        size: usize,
    },
    /// moxcms could not read it, or its tables are over this crate's caps;
    /// or (a PNG's) the decoder dropped it, corrupt or larger than it may
    /// inflate.
    Malformed {
        /// moxcms's account.
        fault: String,
    },
    /// It is not an RGB profile (grey, CMYK, ...).
    NotRgb,
    /// Converting with it would take the decode over `max_working_bytes`.
    OverWorkingBound,
    /// No transform from it to sRGB could be built.
    NoTransform {
        /// moxcms's account.
        fault: String,
    },
}

/// Converts `rgba`, `width` pixels a row, from `icc` to sRGB, as the
/// module's documentation says, and says what was done; or that it failed
/// part way.
pub(crate) fn convert(
    rgba: &mut [u8],
    width: usize,
    icc: &[u8],
) -> Result<Profile, PartlyConverted> {
    if icc.len() > MAX_PROFILE_BYTES {
        return Ok(Profile::Ignored(Ignored::TooLarge { size: icc.len() }));
    }
    let options = ParsingOptions {
        // moxcms refuses a profile of exactly its maximum.
        max_profile_size: MAX_PROFILE_BYTES + 1,
        max_allowed_clut_size: MAX_GRID_POINTS,
        max_allowed_trc_size: MAX_CURVE_ENTRIES,
    };
    let from = match ColorProfile::new_from_slice_with_options(icc, options) {
        Ok(profile) => profile,
        Err(fault) => {
            return Ok(Profile::Ignored(Ignored::Malformed {
                fault: fault.to_string(),
            }));
        }
    };
    if from.color_space != DataColorSpace::Rgb {
        return Ok(Profile::Ignored(Ignored::NotRgb));
    }
    let srgb = ColorProfile::new_srgb();
    let transform = match from.create_transform_8bit(
        Layout::Rgba,
        &srgb,
        Layout::Rgba,
        TransformOptions::default(),
    ) {
        Ok(transform) => transform,
        Err(fault) => {
            return Ok(Profile::Ignored(Ignored::NoTransform {
                fault: fault.to_string(),
            }));
        }
    };
    let probe = probe();
    let mut probed = vec![0; probe.len()];
    if transform.transform(&probe, &mut probed).is_err() {
        return Ok(Profile::Ignored(Ignored::NoTransform {
            fault: "the transform refused the probe".to_owned(),
        }));
    }
    if same_colours(&probe, &probed) {
        return Ok(Profile::AlreadySrgb);
    }
    let stride = width * 4;
    let mut scratch = vec![0; stride];
    for (index, row) in rgba.chunks_exact_mut(stride).enumerate() {
        scratch.copy_from_slice(row);
        if let Err(fault) = transform.transform(&scratch, row) {
            // A failed row is left as it was. Before any row was converted
            // the picture is as decoded, as the label says; after, it is
            // neither, and the picture is refused.
            return if index == 0 {
                Ok(Profile::Ignored(Ignored::NoTransform {
                    fault: fault.to_string(),
                }))
            } else {
                Err(PartlyConverted(fault.to_string()))
            };
        }
    }
    Ok(Profile::Converted)
}

/// A conversion that failed after some rows were converted: the picture is
/// neither as decoded nor sRGB, and is refused
/// ([`crate::DecodeError::ColourConversion`]).
#[derive(Debug)]
pub(crate) struct PartlyConverted(pub(crate) String);

/// The colours a profile is tried on: 17 levels a channel, and every grey.
fn probe() -> Vec<u8> {
    let levels: Vec<u8> = (0..17u16)
        .map(|i| u8::try_from((i * 255).div_ceil(16)).unwrap_or(u8::MAX))
        .collect();
    let mut probe = Vec::with_capacity((17 * 17 * 17 + 256) * 4);
    for &r in &levels {
        for &g in &levels {
            for &b in &levels {
                probe.extend_from_slice(&[r, g, b, u8::MAX]);
            }
        }
    }
    for grey in 0..=u8::MAX {
        probe.extend_from_slice(&[grey, grey, grey, u8::MAX]);
    }
    probe
}

/// Whether every colour of `after` is that of `before` (alpha aside).
fn same_colours(before: &[u8], after: &[u8]) -> bool {
    let (before, _) = before.as_chunks::<4>();
    let (after, _) = after.as_chunks::<4>();
    before.iter().zip(after).all(|(b, a)| b[..3] == a[..3])
}
