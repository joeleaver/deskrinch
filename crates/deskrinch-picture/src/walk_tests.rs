use deskrinch_test_pictures as pictures;
use pictures::fixtures::BASELINE_JPEG;
use pictures::png::{BitDepth, ColorType, encode, with_chunk, with_chunk_at_end};

use super::*;

#[test]
fn a_jpeg_s_application_segments_and_profile_are_summed() {
    assert_eq!(jpeg(BASELINE_JPEG).profile, 0);
    let profile = pictures::icc::display_p3();
    let with = jpeg(&pictures::jpeg::with_profile(BASELINE_JPEG, &profile));
    assert_eq!(with.profile, profile.len());
    let plain = jpeg(BASELINE_JPEG).application;
    // The profile's segment: its signature, two numbers, the profile.
    assert_eq!(with.application, plain + 12 + 2 + profile.len());
    let comment = pictures::jpeg::with_segments(BASELINE_JPEG, 0xfe, &[7; 100], 3);
    assert_eq!(jpeg(&comment).application, plain);
    let exif = jpeg(&pictures::jpeg::with_segments(
        BASELINE_JPEG,
        0xe1,
        b"Exif\0\0abcd",
        2,
    ));
    assert_eq!(exif.application, plain + 2 * 10);
}

#[test]
fn a_png_s_exif_is_found_before_and_after_the_image_data() {
    let bytes = encode(1, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4]);
    assert_eq!(png(&bytes), PngMetadata::default());
    let first = png(&with_chunk(&bytes, *b"eXIf", &[0; 40]));
    assert_eq!((first.exif, first.exif_first), (40, true));
    let last = png(&with_chunk_at_end(&bytes, *b"eXIf", &[0; 40]));
    assert_eq!((last.exif, last.exif_first), (40, false));
    assert!(png(&with_chunk(&bytes, *b"iCCP", b"x\0\0")).profile);
}

#[test]
fn a_gif_s_application_extensions_before_its_frame_are_summed() {
    let frame = pictures::gif::Placed {
        left: 0,
        top: 0,
        width: 2,
        height: 2,
        rgba: vec![9; 16],
    };
    let plain = pictures::gif::encode(2, 2, &[frame]);
    assert_eq!(gif(&plain), 0);
    let with = pictures::gif::with_extension(&plain, 0xff, Some(b"XMP DataXMP"), &[1; 600]);
    // The identifier is its first sub-block.
    assert_eq!(gif(&with), 11 + 600);
    let comment = pictures::gif::with_extension(&plain, 0xfe, None, &[1; 600]);
    assert_eq!(gif(&comment), 0);
}

#[test]
fn a_webp_s_profile_and_exif_are_found_and_exif_after_the_image_is_not() {
    let rgba = [9; 16];
    assert_eq!(
        webp(&pictures::webp::lossless(2, 2, &rgba)),
        WebpMetadata::default()
    );
    let ordered = webp(&pictures::webp::extended(
        2, 2, &rgba, &[5; 300], &[6; 40], &[7; 50],
    ));
    assert_eq!(ordered.profile, 300);
    assert_eq!(ordered.exif_first, None);
    let first = webp(&pictures::webp::with_exif_first(2, 2, &rgba, &[6; 41]));
    assert_eq!(first.exif_first, Some(41));
}

#[test]
fn lengths_running_past_the_bytes_end_each_walk() {
    let mut bytes = encode(1, 1, ColorType::Rgba, BitDepth::Eight, &[1, 2, 3, 4]);
    bytes[33..37].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(png(&bytes), PngMetadata::default());
    // A segment cut short counts what there is of it.
    assert!(jpeg(&BASELINE_JPEG[..30]).application <= 30);
    assert_eq!(gif(b"GIF89a"), 0);
    assert_eq!(
        webp(b"RIFF\0\0\0\0WEBPICCP\xff\xff\xff\xff"),
        WebpMetadata {
            profile: 0xffff_ffff,
            exif_first: None
        }
    );
}
