//! The committed fixture files (see the crate's documentation for how
//! each was made).

/// A 32 × 16 baseline JPEG of the four quadrants, 4:4:4.
pub const BASELINE_JPEG: &[u8] = include_bytes!("../fixtures/baseline.jpg");
/// The same, progressive.
pub const PROGRESSIVE_JPEG: &[u8] = include_bytes!("../fixtures/progressive.jpg");
/// The same, baseline with 4:2:0 chroma subsampling.
pub const SUBSAMPLED_JPEG: &[u8] = include_bytes!("../fixtures/subsampled.jpg");
/// A 16 × 16 grey JPEG of value 100.
pub const GREY_JPEG: &[u8] = include_bytes!("../fixtures/grey.jpg");
/// A 16 × 16 CMYK JPEG (Adobe's inverted CMYK) of (0, 128, 255, 0).
pub const CMYK_JPEG: &[u8] = include_bytes!("../fixtures/cmyk.jpg");
/// A 256 × 256 progressive JPEG of noise, 4:4:4.
pub const PROGRESSIVE_NOISE_JPEG: &[u8] = include_bytes!("../fixtures/progressive-noise-256.jpg");
/// A 32 × 16 lossy WebP of the four quadrants.
pub const LOSSY_WEBP: &[u8] = include_bytes!("../fixtures/lossy.webp");
/// The same with an alpha of 128 throughout.
pub const LOSSY_ALPHA_WEBP: &[u8] = include_bytes!("../fixtures/lossy-alpha.webp");
/// A 256 × 256 lossy WebP of noise with alpha (also noise), quality 60.
pub const LOSSY_ALPHA_NOISE_WEBP: &[u8] = include_bytes!("../fixtures/lossy-alpha-noise-256.webp");
/// A 1024 × 1024 gradient, progressive JPEG, 4:4:4, quality 85.
pub const GRADIENT_PROGRESSIVE_444_JPEG: &[u8] =
    include_bytes!("../fixtures/gradient-1024-progressive-444.jpg");
/// The same gradient, progressive JPEG, 4:2:0.
pub const GRADIENT_PROGRESSIVE_420_JPEG: &[u8] =
    include_bytes!("../fixtures/gradient-1024-progressive-420.jpg");
/// The same gradient, baseline JPEG, 4:2:0.
pub const GRADIENT_BASELINE_JPEG: &[u8] = include_bytes!("../fixtures/gradient-1024-baseline.jpg");
/// A 2048 × 2048 gradient, lossy WebP without alpha, quality 80.
pub const GRADIENT_LOSSY_WEBP: &[u8] = include_bytes!("../fixtures/gradient-2048-lossy.webp");
/// A 1024 × 1024 gradient with an alpha falling from top to bottom, lossy
/// WebP, quality 80.
pub const GRADIENT_LOSSY_ALPHA_WEBP: &[u8] =
    include_bytes!("../fixtures/gradient-1024-lossy-alpha.webp");
