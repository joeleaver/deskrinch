//! Pictures for deskrinch-picture's tests, made in Rust where a Rust
//! encoder can make them, and committed where none can (`fixtures/`).
//!
//! - [`fixtures`]: the committed files: JPEGs (baseline, progressive, 4:2:0,
//!   grey, CMYK, a 256 × 256 progressive of noise) and lossy WebP files, made
//!   with Pillow 12.1 (libjpeg-turbo, libwebp): a 32 × 16 picture of four
//!   quadrants ([`QUADRANTS`]) at quality 95 without chroma subsampling
//!   (`subsampled.jpg`: 4:2:0), a 16 × 16 grey of 100, a 16 × 16 CMYK of
//!   (0, 128, 255, 0), 256 × 256 noise at quality 75 (JPEG) and 60 (WebP,
//!   with noise for alpha too).
//! - [`png`], [`gif`], [`webp`]: encoders for the other formats and layouts.
//! - [`jpeg`]: segments spliced into a JPEG (EXIF orientation, an ICC
//!   profile, repeated scans).
//! - [`icc`]: profiles: moxcms's own (sRGB, Display P3, Adobe RGB, grey)
//!   and lookup-table profiles of any size.
//! - [`hostile`]: what a stranger might send: headers claiming huge sizes,
//!   decompression bombs, too many scans.
//!
//! A picture that cannot be made is a broken test: every helper panics then,
//! as a test's own `unwrap` would.
#![expect(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "test fixtures: a fixture that cannot be made fails the test that asked for it"
)]

pub mod fixtures;
pub mod gif;
pub mod hostile;
pub mod icc;
pub mod jpeg;
pub mod png;
pub mod webp;

/// The four quadrants' colours of the fixtures, top left, top right,
/// bottom left, bottom right.
pub const QUADRANTS: [[u8; 3]; 4] = [[220, 30, 30], [30, 200, 40], [30, 40, 210], [240, 240, 240]];
