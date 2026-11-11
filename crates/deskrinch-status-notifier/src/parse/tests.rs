use std::path::Path;

use zbus::wire::OwnedObjectPath;

use super::*;

fn id() -> ItemId {
    ItemId::new(":1.7", "/StatusNotifierItem")
}

fn read(properties: Properties) -> Item {
    item(id(), properties, &Limits::default())
}

#[test]
fn a_pixmap_is_decoded_from_argb_in_network_order_to_rgba() {
    let argb = [0x80, 0x11, 0x22, 0x33, 0xff, 0x44, 0x55, 0x66];
    let image = decode(2, 1, &argb, 16).unwrap();
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(
        image.rgba(),
        &[0x11, 0x22, 0x33, 0x80, 0x44, 0x55, 0x66, 0xff]
    );
}

#[test]
fn a_pixmap_whose_data_does_not_match_its_size_is_refused() {
    assert_eq!(decode(2, 2, &[0; 15], 16), None);
    assert_eq!(decode(2, 2, &[0; 17], 16), None);
    assert_eq!(decode(1, 1, &[], 16), None);
}

#[test]
fn a_pixmap_out_of_bounds_is_refused_before_anything_is_allocated() {
    assert_eq!(decode(17, 1, &[0; 68], 16), None);
    assert_eq!(decode(0, 1, &[], 16), None);
    assert_eq!(decode(-1, 1, &[], 16), None);
    assert_eq!(decode(i32::MAX, i32::MAX, &[], u32::MAX), None);
}

#[test]
fn an_icon_keeps_its_valid_pixmaps_smallest_first() {
    let pixmaps = vec![(2, 2, vec![0; 16]), (1, 1, vec![0; 3]), (1, 1, vec![0; 4])];
    let item = read(Properties {
        icon_pixmap: Some(pixmaps),
        ..Properties::default()
    });
    let sizes: Vec<_> = item.icon.pixmaps.iter().map(|p| p.width()).collect();
    assert_eq!(sizes, [1, 2]);
}

#[test]
fn an_icon_reads_no_more_pixmaps_than_its_bound() {
    let limits = Limits {
        max_pixmaps: 2,
        ..Limits::default()
    };
    let item = item(
        id(),
        Properties {
            icon_pixmap: Some(vec![(1, 1, vec![0; 4]); 5]),
            ..Properties::default()
        },
        &limits,
    );
    assert_eq!(item.icon.pixmaps.len(), 2);
}

#[test]
fn icon_names_that_are_paths_or_not_names_are_dropped() {
    for bad in ["/usr/share/icons/app.png", "", "a b", "../app"] {
        let item = read(Properties {
            icon_name: Some(bad.into()),
            ..Properties::default()
        });
        assert_eq!(item.icon.name, None, "{bad:?}");
    }
    let item = read(Properties {
        icon_name: Some("mail-unread".into()),
        ..Properties::default()
    });
    assert_eq!(item.icon.name.unwrap().as_str(), "mail-unread");
}

#[test]
fn strings_are_cleaned_and_bounded() {
    let limits = Limits {
        max_text_bytes: 4,
        ..Limits::default()
    };
    let item = item(
        id(),
        Properties {
            title: Some("a\u{7}b\ncdef".into()),
            id: Some("identifier".into()),
            ..Properties::default()
        },
        &limits,
    );
    assert_eq!(item.title, "ab c");
    assert_eq!(item.app_id, "iden");
}

#[test]
fn absent_properties_read_as_their_defaults() {
    let item = read(Properties::default());
    assert_eq!(item.id, id());
    assert_eq!((item.title.as_str(), item.app_id.as_str()), ("", ""));
    assert_eq!(item.status, Status::Active);
    assert_eq!(item.category, Category::ApplicationStatus);
    assert!(item.icon.is_empty() && item.overlay.is_empty() && item.attention.is_empty());
    assert_eq!(item.tooltip, None);
    assert!(!item.has_menu() && !item.item_is_menu);
    assert_eq!(item.icon_theme_path, None);
}

#[test]
fn status_and_category_outside_the_specification_read_as_the_defaults() {
    let item = read(Properties {
        status: Some("Blinking".into()),
        category: Some("Toaster".into()),
        ..Properties::default()
    });
    assert_eq!(item.status, Status::Active);
    assert_eq!(item.category, Category::ApplicationStatus);
    let item = read(Properties {
        status: Some("Passive".into()),
        category: Some("Communications".into()),
        ..Properties::default()
    });
    assert_eq!(item.status, Status::Passive);
    assert_eq!(item.category, Category::Communications);
}

#[test]
fn a_menu_at_the_root_path_is_no_menu() {
    let menu = |path: &str| {
        read(Properties {
            menu: Some(OwnedObjectPath::try_from(path).unwrap()),
            ..Properties::default()
        })
        .has_menu()
    };
    assert!(!menu("/"));
    assert!(menu("/MenuBar"));
}

#[test]
fn the_icon_theme_path_must_be_absolute_and_clean() {
    let theme = |path: &str| {
        read(Properties {
            icon_theme_path: Some(path.into()),
            ..Properties::default()
        })
        .icon_theme_path
    };
    assert_eq!(
        theme("/opt/app/icons").as_deref(),
        Some(Path::new("/opt/app/icons"))
    );
    assert_eq!(theme("icons"), None);
    assert_eq!(theme(""), None);
    assert_eq!(theme("/opt/\u{0}app"), None);
}

#[test]
fn a_tooltip_with_nothing_in_it_is_no_tooltip() {
    let tooltip = |raw: RawToolTip| {
        read(Properties {
            tool_tip: Some(raw),
            ..Properties::default()
        })
        .tooltip
    };
    assert_eq!(
        tooltip((String::new(), Vec::new(), String::new(), String::new())),
        None
    );
    let kept = tooltip((
        String::new(),
        Vec::new(),
        "Title".into(),
        "line one<br>line <b>two</b> &amp; three".into(),
    ))
    .unwrap();
    assert_eq!(kept.title, "Title");
    assert_eq!(kept.description, "line one\nline two & three");
}
