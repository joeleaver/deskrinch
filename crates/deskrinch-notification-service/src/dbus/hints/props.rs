//! Properties of the hints decoder over untrusted input.

use std::collections::HashMap;

use proptest::prelude::*;
use zbus::Variant;
use zbus::wire::serialized::{Context, Data};
use zbus::wire::{LE, to_bytes};

use super::*;
use crate::props::config;

/// The names the decoder knows, and some it does not.
const KEYS: &[&str] = &[
    "urgency",
    "category",
    "desktop-entry",
    "image-data",
    "image_data",
    "image-path",
    "image_path",
    "icon_data",
    "sound-file",
    "sound-name",
    "suppress-sound",
    "transient",
    "resident",
    "action-icons",
    "x",
    "y",
    "x-vendor-hint",
    "",
];

/// The `(iiibiiay)` structure.
type ImageFields = (i32, i32, i32, bool, i32, i32, Vec<u8>);
/// Its first six fields alone.
type Header = (i32, i32, i32, bool, i32, i32);

/// A hint value of any of the types a sender might send. (D-Bus strings
/// cannot hold NUL; malformed input is covered by `arbitrary_bytes_never_panic`.)
#[derive(Debug, Clone)]
enum Value {
    U8(u8),
    I32(i32),
    U32(u32),
    I64(i64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Image(ImageFields),
    /// An image structure missing its pixels.
    Truncated(Header),
    Strings(Vec<String>),
    Nested(Box<Value>),
}

impl Value {
    fn variant(&self) -> Variant<'static> {
        match self.clone() {
            Self::U8(v) => Variant::from(v),
            Self::I32(v) => Variant::from(v),
            Self::U32(v) => Variant::from(v),
            Self::I64(v) => Variant::from(v),
            Self::Bool(v) => Variant::from(v),
            Self::Str(v) => Variant::from(v),
            Self::Bytes(v) => Variant::from(v),
            Self::Image(fields) => Variant::from(fields),
            Self::Truncated(header) => Variant::from(header),
            Self::Strings(v) => Variant::from(v),
            Self::Nested(v) => Variant::new(v.variant()),
        }
    }
}

fn values() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<u8>().prop_map(Value::U8),
        any::<i32>().prop_map(Value::I32),
        any::<u32>().prop_map(Value::U32),
        any::<i64>().prop_map(Value::I64),
        any::<bool>().prop_map(Value::Bool),
        "[^\\x00]{0,64}".prop_map(Value::Str),
        prop::collection::vec(any::<u8>(), 0..256).prop_map(Value::Bytes),
        (
            any::<i32>(),
            any::<i32>(),
            any::<i32>(),
            any::<bool>(),
            any::<i32>(),
            any::<i32>(),
            prop::collection::vec(any::<u8>(), 0..256),
        )
            .prop_map(Value::Image),
        (
            any::<i32>(),
            any::<i32>(),
            any::<i32>(),
            any::<bool>(),
            any::<i32>(),
            any::<i32>()
        )
            .prop_map(Value::Truncated),
        prop::collection::vec("[^\\x00]{0,8}", 0..4).prop_map(Value::Strings),
    ];
    leaf.prop_recursive(2, 8, 1, |inner| {
        inner.prop_map(|v| Value::Nested(Box::new(v)))
    })
}

fn keys() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(KEYS).prop_map(str::to_owned),
        "[^\\x00]{0,16}",
    ]
}

proptest! {
    #![proptest_config(config())]

    /// Any well-formed `a{sv}` decodes, whatever its keys and value types;
    /// each standard hint holds its value only when it was sent with its
    /// specified type.
    #[test]
    fn any_dictionary_decodes(hints in prop::collection::hash_map(keys(), values(), 0..24)) {
        let dict: HashMap<&str, Variant<'_>> =
            hints.iter().map(|(k, v)| (k.as_str(), v.variant())).collect();
        let data = to_bytes(Context::new(LE, 0), &dict).unwrap();
        let (decoded, _) = data.deserialize::<Hints<'_>>().unwrap();
        let typed = |key: &str, ok: fn(&Value) -> bool| hints.get(key).is_some_and(ok);
        prop_assert_eq!(decoded.category.is_some(), typed("category", |v| matches!(v, Value::Str(_))));
        prop_assert_eq!(decoded.transient.is_some(), typed("transient", |v| matches!(v, Value::Bool(_))));
        prop_assert_eq!(decoded.x.is_some(), typed("x", |v| matches!(v, Value::I32(_))));
        prop_assert_eq!(
            decoded.image_data.is_some(),
            typed("image-data", |v| matches!(v, Value::Image(..)))
        );
        let level = match hints.get("urgency") {
            Some(Value::U8(v)) => Some(*v),
            Some(Value::I32(v)) => u8::try_from(*v).ok(),
            Some(Value::U32(v)) => u8::try_from(*v).ok(),
            Some(Value::I64(v)) => u8::try_from(*v).ok(),
            _ => None,
        };
        prop_assert_eq!(decoded.urgency, level);
    }

    /// Bytes that are not a well-formed `a{sv}` are an error, never a panic.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..1024)) {
        let data = Data::new(bytes, Context::new(LE, 0));
        let _result = data.deserialize::<Hints<'_>>();
    }
}
