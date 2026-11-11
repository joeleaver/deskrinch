use std::collections::HashMap;

use zbus::wire::serialized::{Context, Data};
use zbus::wire::{LE, ObjectPath, Variant, to_bytes};

use super::*;

/// `properties` encoded as `GetAll`'s reply body.
fn encode(properties: &HashMap<&str, Variant<'_>>) -> Data<'static, 'static> {
    to_bytes(Context::new(LE, 0), properties).unwrap()
}

fn decode(properties: &HashMap<&str, Variant<'_>>) -> Properties {
    encode(properties).deserialize::<Properties>().unwrap().0
}

fn pixmap(width: i32, height: i32, fill: u8) -> RawPixmap {
    let len = usize::try_from(width * height * 4).unwrap();
    (width, height, vec![fill; len])
}

#[test]
fn every_property_read_is_decoded_into_its_type() {
    let path = ObjectPath::try_from("/MenuBar").unwrap();
    let properties = HashMap::from([
        ("Category", Variant::from("Hardware")),
        ("Id", Variant::from("app")),
        ("Title", Variant::from("App")),
        ("Status", Variant::from("NeedsAttention")),
        ("IconThemePath", Variant::from("/usr/share/app/icons")),
        ("Menu", Variant::from(path)),
        ("ItemIsMenu", Variant::from(true)),
        ("IconName", Variant::from("app")),
        ("IconPixmap", Variant::from(vec![pixmap(1, 1, 7)])),
        ("OverlayIconName", Variant::from("badge")),
        ("OverlayIconPixmap", Variant::from(vec![pixmap(2, 1, 8)])),
        ("AttentionIconName", Variant::from("app-alert")),
        (
            "AttentionIconPixmap",
            Variant::from(Vec::<RawPixmap>::new()),
        ),
        (
            "ToolTip",
            Variant::from((
                "tip".to_owned(),
                vec![pixmap(1, 2, 9)],
                "Title".to_owned(),
                "Body".to_owned(),
            )),
        ),
    ]);
    let p = decode(&properties);
    assert_eq!(p.category.as_deref(), Some("Hardware"));
    assert_eq!(p.id.as_deref(), Some("app"));
    assert_eq!(p.title.as_deref(), Some("App"));
    assert_eq!(p.status.as_deref(), Some("NeedsAttention"));
    assert_eq!(p.icon_theme_path.as_deref(), Some("/usr/share/app/icons"));
    assert_eq!(p.menu.as_deref().map(ObjectPath::as_str), Some("/MenuBar"));
    assert_eq!(p.item_is_menu, Some(true));
    assert_eq!(p.icon_name.as_deref(), Some("app"));
    assert_eq!(p.icon_pixmap, Some(vec![pixmap(1, 1, 7)]));
    assert_eq!(p.overlay_icon_name.as_deref(), Some("badge"));
    assert_eq!(p.overlay_icon_pixmap, Some(vec![pixmap(2, 1, 8)]));
    assert_eq!(p.attention_icon_name.as_deref(), Some("app-alert"));
    assert_eq!(p.attention_icon_pixmap, Some(Vec::new()));
    let tooltip = p.tool_tip.unwrap();
    assert_eq!(
        (tooltip.0.as_str(), tooltip.2.as_str(), tooltip.3.as_str()),
        ("tip", "Title", "Body")
    );
    assert_eq!(tooltip.1, vec![pixmap(1, 2, 9)]);
}

#[test]
fn a_property_of_the_wrong_type_reads_as_absent_and_the_rest_are_kept() {
    let properties = HashMap::from([
        ("Title", Variant::from(7_u32)),
        ("IconPixmap", Variant::from(vec!["not", "pixels"])),
        ("Menu", Variant::from("/MenuBar")),
        ("ItemIsMenu", Variant::from("yes")),
        ("Id", Variant::from("app")),
    ]);
    let p = decode(&properties);
    assert_eq!(p.title, None);
    assert_eq!(p.icon_pixmap, None);
    assert_eq!(p.menu, None);
    assert_eq!(p.item_is_menu, None);
    assert_eq!(p.id.as_deref(), Some("app"));
}

#[test]
fn keys_this_host_does_not_read_are_skipped() {
    let properties = HashMap::from([
        ("XAyatanaLabel", Variant::from("label")),
        ("WindowId", Variant::from(0_i32)),
        ("AttentionMovieName", Variant::from("movie")),
        ("Id", Variant::from("app")),
    ]);
    assert_eq!(
        decode(&properties),
        Properties {
            id: Some("app".into()),
            ..Properties::default()
        }
    );
}

#[test]
fn an_empty_reply_reads_as_no_properties() {
    assert_eq!(decode(&HashMap::new()), Properties::default());
}

#[test]
fn a_body_that_is_not_a_dictionary_is_refused() {
    let data = to_bytes(Context::new(LE, 0), &("Id", 7_u32)).unwrap();
    assert!(data.deserialize::<Properties>().is_err());
}
