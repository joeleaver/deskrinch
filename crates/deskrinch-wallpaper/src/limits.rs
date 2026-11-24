//! What the wallpaper decodes: its formats, its limits, and how a picture
//! over them is said.
//!
//! The limits are constants, not settings: a picture is a file the user
//! chose, but its bytes are anyone's (a download, a shared folder), and
//! `deskrinch-picture` holds every one of them before the memory it
//! guards is taken.

use deskrinch_picture::{DecodeError, Formats, Limits};

/// The formats the wallpaper shows: every one `deskrinch-picture` decodes.
pub(crate) const FORMATS: Formats = Formats::all();

/// The wallpaper's limits on one decode.
///
/// - **64 MiB encoded**: past any photograph or 16K PNG in practice, and the
///   bound on what is read before anything is parsed.
/// - **16384 a side**: 16K, the largest panorama worth a background. Only
///   output-sized pixels, or the picture itself, reach rinch, so a GPU's
///   largest texture is not this bound's business.
/// - **512 MiB decoded**: rinch's own bound on a decoded image
///   (`MAX_DECODED_IMAGE_BYTES`), so no picture rinch showed before the
///   wallpaper decoded its own is refused now for its size.
/// - **1 GiB working**: twice the largest decoded picture, which every
///   format's decode fits in at that size except a progressive JPEG beyond
///   about a hundred megapixels, whose coefficients need more; that one is
///   refused, with what it needs.
pub(crate) const LIMITS: Limits = Limits {
    max_encoded_bytes: 64 << 20,
    max_side: 16_384,
    max_decoded_bytes: 512 << 20,
    max_working_bytes: 1 << 30,
};

/// Why a picture is not shown, in the words the wallpaper's warning uses:
/// for a limit, what the picture needs and what the wallpaper allows.
pub(crate) fn why(error: &DecodeError) -> String {
    match error {
        DecodeError::EncodedTooLarge { size, max } => {
            format!("the file is {size} bytes; the wallpaper reads files of up to {max}")
        }
        DecodeError::Dimensions {
            width,
            height,
            max_side,
        } => format!(
            "it is {width}×{height} pixels; the wallpaper shows pictures up to {max_side} a side"
        ),
        DecodeError::DecodedTooLarge { width, height, max } => format!(
            "it is {width}×{height} pixels, more than {max} bytes decoded, which is what the \
             wallpaper allows"
        ),
        DecodeError::WorkingTooLarge {
            needed: Some(needed),
            max,
        } => format!("decoding it needs {needed} bytes at its peak; the wallpaper allows {max}"),
        DecodeError::WorkingTooLarge { needed: None, max } => {
            format!("decoding it needs more bytes than can be counted; the wallpaper allows {max}")
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_says_what_the_picture_needs_and_what_is_allowed() {
        let side = why(&DecodeError::Dimensions {
            width: 20_000,
            height: 100,
            max_side: LIMITS.max_side,
        });
        assert_eq!(
            side,
            "it is 20000×100 pixels; the wallpaper shows pictures up to 16384 a side"
        );
        let working = why(&DecodeError::WorkingTooLarge {
            needed: Some(2 << 30),
            max: LIMITS.max_working_bytes,
        });
        assert_eq!(
            working,
            "decoding it needs 2147483648 bytes at its peak; the wallpaper allows 1073741824"
        );
        assert!(why(&DecodeError::NotRecognised).contains("not a format"));
    }

    #[test]
    fn the_decoded_bound_is_rinchs_own() {
        assert_eq!(
            u64::try_from(LIMITS.max_decoded_bytes).unwrap(),
            rinch::image::MAX_DECODED_IMAGE_BYTES
        );
        assert_eq!(LIMITS.max_working_bytes, 2 * LIMITS.max_decoded_bytes);
    }
}
