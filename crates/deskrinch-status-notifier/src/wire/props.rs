//! Properties of reading an untrusted `GetAll` reply.

use std::collections::HashMap;

use proptest::prelude::*;
use zbus::wire::serialized::{Context, Data};
use zbus::wire::{LE, Variant};

use super::*;
use deskrinch_untrusted::fuzz::config;

/// A value an item might send for a property, of one type or another.
#[derive(Debug, Clone)]
enum Sent {
    Text(String),
    Number(u32),
    Flag(bool),
    Bytes(Vec<u8>),
    Pixmaps(Vec<RawPixmap>),
}

impl Sent {
    fn variant(&self) -> Variant<'_> {
        match self {
            Self::Text(s) => Variant::from(s.as_str()),
            Self::Number(n) => Variant::from(*n),
            Self::Flag(b) => Variant::from(*b),
            Self::Bytes(b) => Variant::from(b.clone()),
            Self::Pixmaps(p) => Variant::from(p.clone()),
        }
    }
}

fn sent() -> impl Strategy<Value = Sent> {
    let pixmap = (
        any::<i32>(),
        any::<i32>(),
        prop::collection::vec(any::<u8>(), 0..64),
    );
    prop_oneof![
        any::<String>().prop_map(Sent::Text),
        any::<u32>().prop_map(Sent::Number),
        any::<bool>().prop_map(Sent::Flag),
        prop::collection::vec(any::<u8>(), 0..256).prop_map(Sent::Bytes),
        prop::collection::vec(pixmap, 0..4).prop_map(Sent::Pixmaps),
    ]
}

fn key() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec![
            "Category",
            "Id",
            "Title",
            "Status",
            "IconThemePath",
            "Menu",
            "ItemIsMenu",
            "IconName",
            "IconPixmap",
            "OverlayIconName",
            "OverlayIconPixmap",
            "AttentionIconName",
            "AttentionIconPixmap",
            "ToolTip",
        ])
        .prop_map(str::to_owned),
        any::<String>(),
    ]
}

fn encode(properties: &HashMap<String, Sent>) -> Data<'static, 'static> {
    let variants: HashMap<&str, Variant<'_>> = properties
        .iter()
        .map(|(k, v)| (k.as_str(), v.variant()))
        .collect();
    zbus::wire::to_bytes(Context::new(LE, 0), &variants).unwrap()
}

proptest! {
    #![proptest_config(config())]

    /// Any dictionary of properties decodes, and a property is present
    /// exactly when it was sent with its own type.
    #[test]
    fn any_dictionary_decodes_keeping_only_well_typed_properties(
        properties in prop::collection::hash_map(key(), sent(), 0..12),
    ) {
        let decoded = encode(&properties).deserialize::<Properties>().unwrap().0;
        let text = |key: &str| match properties.get(key) {
            Some(Sent::Text(s)) => Some(s.clone()),
            _ => None,
        };
        let pixmaps = |key: &str| match properties.get(key) {
            Some(Sent::Pixmaps(p)) => Some(p.clone()),
            _ => None,
        };
        prop_assert_eq!(decoded.title, text("Title"));
        prop_assert_eq!(decoded.icon_name, text("IconName"));
        prop_assert_eq!(decoded.icon_pixmap, pixmaps("IconPixmap"));
        prop_assert_eq!(decoded.attention_icon_pixmap, pixmaps("AttentionIconPixmap"));
        let flag = match properties.get("ItemIsMenu") {
            Some(Sent::Flag(b)) => Some(*b),
            _ => None,
        };
        prop_assert_eq!(decoded.item_is_menu, flag);
        // Nothing sent here has the types of these two.
        prop_assert_eq!(decoded.menu, None);
        prop_assert_eq!(decoded.tool_tip, None);
    }

    /// Arbitrary bytes are refused or decoded, never a panic.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let data = Data::new(bytes, Context::new(LE, 0));
        let _outcome = data.deserialize::<Properties>();
    }
}
