use std::collections::HashMap;

use zbus::Variant;
use zbus::wire::serialized::{Context, Data};
use zbus::wire::{LE, to_bytes};

use super::*;

fn encode(hints: &HashMap<&str, Variant<'_>>) -> Data<'static, 'static> {
    to_bytes(Context::new(LE, 0), hints).unwrap()
}

fn decode<'a>(data: &'a Data<'static, 'static>) -> Hints<'a> {
    data.deserialize::<Hints<'a>>().unwrap().0
}

#[test]
fn standard_hints_are_read_with_their_types() {
    let pixels = vec![1u8, 2, 3, 4];
    let image = (1i32, 1i32, 4i32, true, 8i32, 4i32, pixels);
    let hints = HashMap::from([
        ("urgency", Variant::from(2u8)),
        ("category", Variant::from("im.received")),
        ("desktop-entry", Variant::from("org.example.Chat")),
        ("image-data", Variant::from(image.clone())),
        ("image_data", Variant::from(image.clone())),
        ("icon_data", Variant::from(image)),
        ("image-path", Variant::from("/a.png")),
        ("image_path", Variant::from("/b.png")),
        ("sound-file", Variant::from("/s.oga")),
        ("sound-name", Variant::from("message-new-instant")),
        ("suppress-sound", Variant::from(true)),
        ("transient", Variant::from(true)),
        ("resident", Variant::from(false)),
        ("action-icons", Variant::from(true)),
        ("x", Variant::from(12i32)),
        ("y", Variant::from(-3i32)),
    ]);
    let data = encode(&hints);
    let got = decode(&data);
    let raw = RawImage {
        width: 1,
        height: 1,
        rowstride: 4,
        has_alpha: true,
        bits_per_sample: 8,
        channels: 4,
        data: &[1, 2, 3, 4],
    };
    assert_eq!(
        got,
        Hints {
            urgency: Some(2),
            category: Some("im.received"),
            desktop_entry: Some("org.example.Chat"),
            image_data: Some(raw),
            image_data_legacy: Some(raw),
            image_path: Some("/a.png"),
            image_path_legacy: Some("/b.png"),
            icon_data: Some(raw),
            sound_file: Some("/s.oga"),
            sound_name: Some("message-new-instant"),
            suppress_sound: Some(true),
            transient: Some(true),
            resident: Some(false),
            action_icons: Some(true),
            x: Some(12),
            y: Some(-3),
        }
    );
}

#[test]
fn wrong_types_and_unknown_hints_are_skipped() {
    let hints = HashMap::from([
        ("category", Variant::from(5u32)),
        ("transient", Variant::from("yes")),
        ("x", Variant::from(1u32)),
        (
            "image-data",
            Variant::from((1i32, 1i32, 4i32, true, 8i32, 4i32)),
        ),
        ("x-vendor-thing", Variant::from(vec![1u8, 2, 3])),
        ("y", Variant::from(4i32)),
    ]);
    let data = encode(&hints);
    assert_eq!(
        decode(&data),
        Hints {
            y: Some(4),
            ..Hints::default()
        }
    );
}

#[test]
fn urgency_is_accepted_from_any_integer_that_fits() {
    for (value, expected) in [
        (Variant::from(1u8), Some(1)),
        (Variant::from(2i32), Some(2)),
        (Variant::from(0u32), Some(0)),
        (Variant::from(1i64), Some(1)),
        (Variant::from(-1i32), None),
        (Variant::from(300u32), None),
        (Variant::from("2"), None),
    ] {
        let data = encode(&HashMap::from([("urgency", value)]));
        assert_eq!(decode(&data).urgency, expected);
    }
}

#[test]
fn a_variant_inside_the_variant_is_not_unwrapped() {
    let data = encode(&HashMap::from([(
        "category",
        Variant::new(Variant::from("x")),
    )]));
    assert_eq!(decode(&data).category, None);
}
