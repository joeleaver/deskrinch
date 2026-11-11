//! The item's properties as they arrive: `GetAll`'s `a{sv}`, read straight
//! into typed fields.
//!
//! Reading the reply into generic variants first would hold every byte of a
//! pixmap as a variant of its own, tens of times the reply's size; here each
//! property is decoded into its own type as it is read. A property of an
//! unexpected type is skipped without being decoded and reads as absent;
//! keys this host does not read are skipped the same way. A key given twice
//! keeps its last value.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use zbus::wire::{OwnedObjectPath, Signature, Type};

/// One pixmap as sent: width, height, ARGB32 pixels in network byte order.
pub(crate) type RawPixmap = (i32, i32, Vec<u8>);

/// A `ToolTip` as sent: icon name, pixmaps, title, description.
pub(crate) type RawToolTip = (String, Vec<RawPixmap>, String, String);

/// The properties of `org.kde.StatusNotifierItem` this host reads; `None`
/// for each one absent or of the wrong type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Properties {
    pub(crate) category: Option<String>,
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) icon_theme_path: Option<String>,
    pub(crate) menu: Option<OwnedObjectPath>,
    pub(crate) item_is_menu: Option<bool>,
    pub(crate) icon_name: Option<String>,
    pub(crate) icon_pixmap: Option<Vec<RawPixmap>>,
    pub(crate) overlay_icon_name: Option<String>,
    pub(crate) overlay_icon_pixmap: Option<Vec<RawPixmap>>,
    pub(crate) attention_icon_name: Option<String>,
    pub(crate) attention_icon_pixmap: Option<Vec<RawPixmap>>,
    pub(crate) tool_tip: Option<RawToolTip>,
}

impl Type for Properties {
    const SIGNATURE: &'static Signature =
        <std::collections::HashMap<String, zbus::wire::OwnedVariant> as Type>::SIGNATURE;
}

impl<'de> serde::Deserialize<'de> for Properties {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(PropertiesVisitor)
    }
}

struct PropertiesVisitor;

impl<'de> Visitor<'de> for PropertiesVisitor {
    type Value = Properties;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a{sv} of StatusNotifierItem properties")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Properties, A::Error> {
        fn take<'de, A: MapAccess<'de>, T: Type + de::Deserialize<'de>>(
            map: &mut A,
        ) -> Result<Option<T>, A::Error> {
            map.next_value::<Lenient<T>>().map(|v| v.0)
        }
        let mut p = Properties::default();
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "Category" => p.category = take(&mut map)?,
                "Id" => p.id = take(&mut map)?,
                "Title" => p.title = take(&mut map)?,
                "Status" => p.status = take(&mut map)?,
                "IconThemePath" => p.icon_theme_path = take(&mut map)?,
                "Menu" => p.menu = take(&mut map)?,
                "ItemIsMenu" => p.item_is_menu = take(&mut map)?,
                "IconName" => p.icon_name = take(&mut map)?,
                "IconPixmap" => p.icon_pixmap = take(&mut map)?,
                "OverlayIconName" => p.overlay_icon_name = take(&mut map)?,
                "OverlayIconPixmap" => p.overlay_icon_pixmap = take(&mut map)?,
                "AttentionIconName" => p.attention_icon_name = take(&mut map)?,
                "AttentionIconPixmap" => p.attention_icon_pixmap = take(&mut map)?,
                "ToolTip" => p.tool_tip = take(&mut map)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(p)
    }
}

/// A variant's value when its signature is `T`'s; `None`, skipped without
/// decoding, when it is anything else.
pub(crate) struct Lenient<T>(pub(crate) Option<T>);

impl<'de, T: Type + de::Deserialize<'de>> de::Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A variant reads as the pair (signature, value), as zbus's own
        // `as_variant` reads it.
        deserializer.deserialize_struct(
            "Variant",
            &["signature", "value"],
            LenientVisitor(PhantomData),
        )
    }
}

struct LenientVisitor<T>(PhantomData<T>);

impl<'de, T: Type + de::Deserialize<'de>> Visitor<'de> for LenientVisitor<T> {
    type Value = Lenient<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a variant")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Lenient<T>, A::Error> {
        let signature: Signature = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        if &signature == T::SIGNATURE {
            let value = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(1, &self))?;
            Ok(Lenient(Some(value)))
        } else {
            seq.next_element::<IgnoredAny>()?;
            Ok(Lenient(None))
        }
    }
}

pub(crate) mod layout;
#[cfg(test)]
mod layout_props;
#[cfg(test)]
mod props;
#[cfg(test)]
mod tests;
