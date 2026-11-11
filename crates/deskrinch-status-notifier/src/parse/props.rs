//! Properties of turning untrusted properties into an item.

use proptest::prelude::*;

use super::*;
use deskrinch_untrusted::fuzz::config;

const MAX_SIDE: u32 = 32;

/// An `i32` a sender might send: anything, with the edges and the small
/// values that make a plausible pixmap more likely.
fn int() -> impl Strategy<Value = i32> {
    prop_oneof![any::<i32>(), -2..40i32, Just(i32::MAX), Just(i32::MIN)]
}

fn pixmaps() -> impl Strategy<Value = Vec<RawPixmap>> {
    prop::collection::vec(
        (int(), int(), prop::collection::vec(any::<u8>(), 0..5000)),
        0..12,
    )
}

fn limits() -> Limits {
    Limits {
        max_text_bytes: 16,
        max_description_bytes: 32,
        max_reference_bytes: 24,
        max_pixmaps: 4,
        max_image_side: MAX_SIDE,
        ..Limits::default()
    }
}

fn icon_within(icon: &Icon, limits: &Limits) -> Result<(), TestCaseError> {
    prop_assert!(icon.pixmaps.len() <= limits.max_pixmaps);
    for p in &icon.pixmaps {
        prop_assert!((1..=MAX_SIDE).contains(&p.width()));
        prop_assert!((1..=MAX_SIDE).contains(&p.height()));
        prop_assert_eq!(p.rgba().len(), p.width() as usize * p.height() as usize * 4);
    }
    if let Some(name) = &icon.name {
        prop_assert!(name.as_str().len() <= limits.max_reference_bytes);
    }
    Ok(())
}

proptest! {
    #![proptest_config(config())]

    /// Whatever the item sends, the item read from it is within bounds.
    #[test]
    fn any_properties_read_within_bounds(
        title in proptest::option::of(any::<String>()),
        app_id in proptest::option::of(any::<String>()),
        icon_name in proptest::option::of(any::<String>()),
        theme in proptest::option::of(any::<String>()),
        icon_pixmap in proptest::option::of(pixmaps()),
        tool_tip in proptest::option::of((any::<String>(), pixmaps(), any::<String>(), any::<String>())),
    ) {
        let limits = limits();
        let properties = Properties {
            title,
            id: app_id,
            icon_name,
            icon_theme_path: theme,
            icon_pixmap,
            tool_tip,
            ..Properties::default()
        };
        let item = item(ItemId::new(":1.1", "/StatusNotifierItem"), properties, &limits);
        prop_assert!(item.title.len() <= limits.max_text_bytes);
        prop_assert!(!item.title.chars().any(char::is_control));
        prop_assert!(item.app_id.len() <= limits.max_text_bytes);
        icon_within(&item.icon, &limits)?;
        if let Some(path) = &item.icon_theme_path {
            prop_assert!(path.is_absolute());
            prop_assert!(path.as_os_str().len() <= limits.max_reference_bytes);
        }
        if let Some(tooltip) = &item.tooltip {
            icon_within(&tooltip.icon, &limits)?;
            prop_assert!(tooltip.title.len() <= limits.max_text_bytes);
            prop_assert!(tooltip.description.len() <= limits.max_description_bytes);
        }
    }

    /// Any pixmap is refused or decodes to exactly its pixels, channel by
    /// channel.
    #[test]
    fn any_pixmap_decodes_exactly_or_is_refused(
        width in int(),
        height in int(),
        argb in prop::collection::vec(any::<u8>(), 0..5000),
    ) {
        if let Some(image) = decode(width, height, &argb, MAX_SIDE) {
            prop_assert_eq!(i64::from(image.width()), i64::from(width));
            prop_assert_eq!(i64::from(image.height()), i64::from(height));
            prop_assert_eq!(image.rgba().len(), argb.len());
            for (out, input) in image.rgba().chunks(4).zip(argb.chunks(4)) {
                prop_assert_eq!(out, &[input[1], input[2], input[3], input[0]]);
            }
        }
    }
}
