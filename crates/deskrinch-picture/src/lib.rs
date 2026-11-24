//! Pictures decoded under the caller's limits into straight RGBA8, sRGB,
//! upright: what the shell does with a picture it did not make (a
//! notification's image, a tray menu's icon, the wallpaper).
//!
//! # What it decodes
//!
//! PNG (APNG: its default image), JPEG (baseline and progressive; grey,
//! YCbCr, RGB, CMYK, YCCK), GIF (the first frame) and WebP (lossy,
//! lossless; an animation's first frame), each found from its signature,
//! never from a file name ([`Format::sniff`]). Anything else, an SVG
//! included, is [`DecodeError::NotRecognised`]. The caller names the
//! formats it accepts ([`Formats`]); one it does not is
//! [`DecodeError::NotAccepted`].
//!
//! # The limits
//!
//! Every bound is the caller's ([`Limits`]), and each is checked before the
//! memory it guards is taken:
//!
//! - the encoded size, before anything is read past it ([`decode_file`]
//!   reads at most that and one byte more, from a regular file only);
//! - the width and height, from the header;
//! - the result, width × height × 4, from the header;
//! - the working bound: everything the decode allocates at its peak, the
//!   result included and the input not. Each format's decoder says from
//!   its header what it will need (the `png`, `jpeg`, `gif` and `webp`
//!   modules' documentation), with the orientation's second buffer and the
//!   colour conversion's tables, and a picture that would need more is
//!   refused before its buffers exist.
//!
//! The process's peak during a decode is therefore the input (at most the
//! encoded bound) plus the working bound.
//!
//! What a decoder keeps of the file's metadata (the EXIF, the XMP, the
//! colour profile, a GIF's application extensions) is charged to the
//! working bound at what it holds: from the decoder's own header read where
//! it says, and otherwise from a walk over the container's length-prefixed
//! blocks, which reads lengths and signatures only. Nothing is capped by a
//! number the caller did not set, but the colour profile
//! ([`MAX_PROFILE_BYTES`]), whose excess is ignored and said to be.
//!
//! A picture refused for a limit may have allocated, before its refusal,
//! what its format's header read keeps of the file's metadata: at most
//! twice the encoded size (a JPEG decoder's copy and the copy of its EXIF it
//! hands over) and a PNG's inflated colour profile, at most
//! [`MAX_PROFILE_BYTES`]. The working-bound test holds refused pictures
//! that are mostly metadata to that. The decoders are used directly,
//! not through `image`, because each lets its caller see more of the
//! header (a JPEG's components and whether it is progressive, a WebP's
//! kind) or bound more of what it allocates (`png`'s and `gif`'s own
//! limits, zune-jpeg's scan cap) than `image` passes on. Where a bound
//! rests on how a decoder allocates rather than on a limit it enforces
//! (zune-jpeg's rows and coefficients, image-webp's planes), the module
//! says so, and the working-bound test (`tests/working_bound.rs`) fails
//! when an update changes it.
//!
//! Time follows from the limits: a decode's work grows with its pixels
//! and, for a progressive JPEG, its scans ([`MAX_SCANS`]). The crate owns
//! no thread; the caller chooses where a decode runs.
//!
//! # What the result is
//!
//! A [`Decoded`]: the [`Image`](deskrinch_untrusted::Image), its format,
//! and what happened to its colour profile ([`Profile`]): an embedded RGB
//! ICC profile is converted to sRGB, an sRGB one is recognised and changes
//! nothing, and one that cannot be used is ignored with its reason
//! ([`Ignored`]).
//!
//! # Orientation
//!
//! A picture's EXIF orientation is applied when the EXIF comes before the
//! image data, and not when it comes after: the CSS Images specification
//! has user agents ignore metadata that changes the layout once the image
//! data has begun, as Chrome and Firefox do for WebP and Safari for PNG.
//! So a JPEG's (APP1, always before its scans) is applied, a PNG's `eXIf`
//! before its `IDAT` is and one after it is not, and a WebP's EXIF chunk,
//! which its specification puts after the image data, is not applied.
//!
//! # Fitting
//!
//! [`fit`] fits a decoded picture to a target size, as the wallpaper fits
//! one to an output ([`Fit`]: cover, contain, stretch, centre, tile), and
//! [`crop`] takes a part of one as it is, under the same kind of limits.
//!
//! # Example
//!
//! ```
//! use deskrinch_picture::{DecodeError, Formats, Limits, decode};
//!
//! let limits = Limits {
//!     max_encoded_bytes: 1 << 20,
//!     max_side: 512,
//!     max_decoded_bytes: 1 << 20,
//!     max_working_bytes: 16 << 20,
//! };
//! assert!(matches!(
//!     decode(b"<svg/>", Formats::all(), &limits),
//!     Err(DecodeError::NotRecognised)
//! ));
//! ```

mod colour;
mod decode;
mod error;
mod fit;
mod gif;
mod jpeg;
mod limits;
mod orient;
mod png;
mod rgba;
mod walk;
mod webp;

pub use colour::{Ignored, MAX_CURVE_ENTRIES, MAX_GRID_POINTS, MAX_PROFILE_BYTES, Profile};
pub use decode::{Decoded, decode, decode_file};
pub use error::DecodeError;
pub use fit::{Fit, FitError, crop, fit};
pub use jpeg::MAX_SCANS;
pub use limits::{Format, Formats, Limits};
