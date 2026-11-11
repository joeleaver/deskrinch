//! Properties of image validation over untrusted input.

use proptest::prelude::*;

use super::*;
use deskrinch_untrusted::fuzz::config;

const MAX_SIDE: u32 = 64;
const MAX_BYTES: usize = 8 * 1024;

/// An `i32` a sender might send: anything, with the edges and the small
/// values that make a plausible image more likely.
fn int() -> impl Strategy<Value = i32> {
    prop_oneof![any::<i32>(), -2..80i32, Just(i32::MAX), Just(i32::MIN),]
}

proptest! {
    #![proptest_config(config())]

    /// Any `(iiibiiay)` either is refused or decodes to exactly
    /// `width × height` RGBA pixels within both bounds, never panicking or
    /// allocating more than the bounds allow.
    #[test]
    fn any_image_decodes_within_bounds_or_is_refused(
        width in int(),
        height in int(),
        rowstride in int(),
        has_alpha in any::<bool>(),
        bits_per_sample in prop_oneof![Just(8), int()],
        channels in prop_oneof![Just(3), Just(4), int()],
        data in prop::collection::vec(any::<u8>(), 0..20_000),
    ) {
        let raw = RawImage { width, height, rowstride, has_alpha, bits_per_sample, channels, data: &data };
        if let Ok(image) = decode_image(&raw, MAX_SIDE, MAX_BYTES) {
            prop_assert!((1..=MAX_SIDE).contains(&image.width()));
            prop_assert!((1..=MAX_SIDE).contains(&image.height()));
            let len = image.width() as usize * image.height() as usize * 4;
            prop_assert_eq!(image.rgba().len(), len);
            prop_assert!(len <= MAX_BYTES);
            prop_assert_eq!(bits_per_sample, 8);
        }
    }

    /// A well-formed image round-trips its pixels, whatever the padding.
    #[test]
    fn valid_images_keep_their_pixels(
        width in 1..=MAX_SIDE,
        height in 1..=MAX_SIDE,
        padding in 0..16usize,
        has_alpha in any::<bool>(),
        seed in any::<u8>(),
    ) {
        let channels = if has_alpha { 4 } else { 3 };
        let row = width as usize * channels;
        let stride = row + padding;
        let data: Vec<u8> = (0..stride * height as usize)
            .map(|i| seed.wrapping_add(u8::try_from(i % 251).unwrap()))
            .collect();
        let raw = RawImage {
            width: i32::try_from(width).unwrap(),
            height: i32::try_from(height).unwrap(),
            rowstride: i32::try_from(stride).unwrap(),
            has_alpha,
            bits_per_sample: 8,
            channels: i32::try_from(channels).unwrap(),
            data: &data,
        };
        let image = decode_image(&raw, MAX_SIDE, usize::MAX).unwrap();
        for (y, out_row) in image.rgba().chunks(width as usize * 4).enumerate() {
            let in_row = &data[y * stride..y * stride + row];
            for (out_px, in_px) in out_row.chunks(4).zip(in_row.chunks(channels)) {
                prop_assert_eq!(&out_px[..3], &in_px[..3]);
                prop_assert_eq!(out_px[3], if has_alpha { in_px[3] } else { u8::MAX });
            }
        }
    }
}
