use super::*;

fn raw(width: i32, height: i32, rowstride: i32, has_alpha: bool, data: &[u8]) -> RawImage<'_> {
    RawImage {
        width,
        height,
        rowstride,
        has_alpha,
        bits_per_sample: 8,
        channels: if has_alpha { 4 } else { 3 },
        data,
    }
}

/// Decodes with at most 64 pixels a side and no lower byte bound.
fn decode(raw: &RawImage<'_>) -> Result<Image, ImageError> {
    Image::decode(raw, 64, 64 * 64 * 4)
}

#[test]
fn an_image_over_the_byte_bound_is_refused_before_allocating() {
    // Claims 4096×4096 RGBA (64 MiB) but carries four bytes; the size check
    // comes before the length check, so nothing is allocated for it.
    let huge = raw(4096, 4096, 4096 * 4, true, &[0; 4]);
    assert_eq!(
        Image::decode(&huge, 4096, 16 * 1024 * 1024),
        Err(ImageError::TooLarge {
            width: 4096,
            height: 4096
        })
    );
    // A full-HD screenshot fits the defaults.
    let limits = crate::limits::Limits::default();
    let data = vec![0; 1920 * 1080 * 4];
    let full_hd = raw(1920, 1080, 1920 * 4, true, &data);
    let image = Image::decode(&full_hd, limits.max_image_side, limits.max_image_bytes).unwrap();
    assert_eq!((image.width(), image.height()), (1920, 1080));
}

#[test]
fn rgba_is_copied_row_by_row_skipping_padding() {
    // 2×2 RGBA with a stride of 12: 4 bytes of padding after each row; the
    // last row is unpadded.
    let data = [
        1, 2, 3, 4, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE, //
        9, 10, 11, 12, 13, 14, 15, 16,
    ];
    let image = decode(&raw(2, 2, 12, true, &data)).unwrap();
    assert_eq!((image.width(), image.height()), (2, 2));
    assert_eq!(
        image.rgba(),
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    );
}

#[test]
fn rgb_gains_an_opaque_alpha_channel() {
    let data = [1, 2, 3, 4, 5, 6, 0, 0];
    let image = decode(&raw(2, 1, 8, false, &data)).unwrap();
    assert_eq!(image.rgba(), &[1, 2, 3, 255, 4, 5, 6, 255]);
}

#[test]
fn a_fully_padded_last_row_is_accepted() {
    let data = [0; 16];
    assert!(decode(&raw(1, 2, 8, true, &data)).is_ok());
}

#[test]
fn bad_dimensions_are_refused() {
    for (w, h) in [
        (0, 1),
        (1, 0),
        (-1, 1),
        (65, 1),
        (1, 65),
        (i32::MAX, i32::MAX),
    ] {
        let err = decode(&raw(w, h, 4, true, &[0; 4])).unwrap_err();
        assert_eq!(
            err,
            ImageError::Dimensions {
                width: w,
                height: h
            }
        );
    }
}

#[test]
fn bits_per_sample_other_than_eight_are_refused() {
    let mut image = raw(1, 1, 4, true, &[0; 4]);
    image.bits_per_sample = 16;
    assert_eq!(decode(&image), Err(ImageError::BitsPerSample(16)));
}

#[test]
fn channels_must_match_alpha() {
    let mut image = raw(1, 1, 4, true, &[0; 4]);
    image.channels = 3;
    assert!(matches!(decode(&image), Err(ImageError::Channels { .. })));
    let mut image = raw(1, 1, 4, false, &[0; 4]);
    image.channels = 4;
    assert!(matches!(decode(&image), Err(ImageError::Channels { .. })));
}

#[test]
fn rowstride_shorter_than_a_row_is_refused() {
    assert_eq!(
        decode(&raw(2, 1, 7, true, &[0; 8])),
        Err(ImageError::Rowstride(7))
    );
    assert_eq!(
        decode(&raw(1, 1, -4, true, &[0; 4])),
        Err(ImageError::Rowstride(-4))
    );
}

#[test]
fn data_length_must_fit_the_layout() {
    // 2×2 RGBA, stride 8: between 16 and 16 bytes.
    assert!(matches!(
        decode(&raw(2, 2, 8, true, &[0; 15])),
        Err(ImageError::Length {
            expected: 16,
            actual: 15
        })
    ));
    assert!(matches!(
        decode(&raw(2, 2, 8, true, &[0; 17])),
        Err(ImageError::Length { .. })
    ));
    // A huge stride cannot make the arithmetic overflow.
    assert!(decode(&raw(64, 64, i32::MAX, true, &[0; 4])).is_err());
}

#[test]
fn references_parse_as_files_or_icon_names() {
    let file = |p: &str| Some(ImageSource::File(PathBuf::from(p)));
    assert_eq!(
        ImageSource::from_reference("/usr/share/a.png", 64),
        file("/usr/share/a.png")
    );
    assert_eq!(
        ImageSource::from_reference("file:///tmp/a%20b.png", 64),
        file("/tmp/a b.png")
    );
    // An escape that is not one stays literal.
    assert_eq!(
        ImageSource::from_reference("file:///tmp/%zz%2", 64),
        file("/tmp/%zz%2")
    );
    assert_eq!(
        ImageSource::from_reference("file://localhost/tmp/a.png", 64),
        file("/tmp/a.png")
    );
    assert_eq!(
        ImageSource::from_reference("dialog-information", 64),
        Some(ImageSource::Icon(IconName("dialog-information".into())))
    );
}

#[test]
fn bad_references_are_refused() {
    for reference in [
        "",
        "relative/path.png",
        "..",
        "file://host/tmp/a.png",
        "file://tmp/a.png",
        "file:///tmp/a.png?x",
        "file:///tmp/a.png#x",
        "file:///tmp/%00",
        "https://example.org/a.png",
        "icon name",
        "icon\u{7}",
    ] {
        assert_eq!(
            ImageSource::from_reference(reference, 64),
            None,
            "{reference:?}"
        );
    }
    assert_eq!(ImageSource::from_reference("abcdef", 5), None);
}
