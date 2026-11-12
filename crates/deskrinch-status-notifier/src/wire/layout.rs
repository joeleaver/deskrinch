//! A menu's layout as `GetLayout` returns it, `(u(ia{sv}av))`: the
//! revision, then the root entry, each entry an id, its properties and its
//! children (each a variant holding an entry).
//!
//! It is read straight into typed entries, holding the menu's structural
//! bounds as it goes: reading stops at the first entry too deep, or one too
//! many, so a hostile layout costs no more than its bounds. Properties are
//! decoded as an item's are: one of an unexpected type reads as absent,
//! and unknown ones are skipped without being decoded.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use zbus::wire::serialized::Data;
use zbus::wire::{DynamicType, OwnedVariant, Signature, Type};

use super::Lenient;

/// One entry as sent, with its children.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawEntry {
    pub(crate) id: i32,
    pub(crate) properties: RawProperties,
    pub(crate) children: Vec<RawEntry>,
}

/// The entry properties this host reads; `None` for each one absent or of
/// the wrong type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawProperties {
    pub(crate) kind: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) enabled: Option<bool>,
    pub(crate) visible: Option<bool>,
    pub(crate) icon_name: Option<String>,
    pub(crate) icon_data: Option<Vec<u8>>,
    pub(crate) toggle_type: Option<String>,
    pub(crate) toggle_state: Option<i32>,
    pub(crate) children_display: Option<String>,
    pub(crate) disposition: Option<String>,
    pub(crate) shortcut: Option<Vec<Vec<String>>>,
}

/// The structural bounds a layout is read within.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bounds {
    /// Levels of submenus below the root's own entries.
    pub(crate) depth: usize,
    /// Entries in all, the root not counted.
    pub(crate) entries: usize,
    /// Children of one entry.
    pub(crate) per_level: usize,
}

/// Why a layout was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Deeper, or with more entries, than its bounds.
    OverBound,
    /// Not a layout.
    Malformed,
}

/// An entry's signature, `(ia{sv}av)`, which each child's variant must
/// have.
type EntryType = (i32, HashMap<String, OwnedVariant>, Vec<OwnedVariant>);
const ENTRY: &Signature = <EntryType as Type>::SIGNATURE;
/// `GetLayout`'s reply, `(u(ia{sv}av))`: a revision and the root entry.
const REPLY: &Signature = <(u32, EntryType) as Type>::SIGNATURE;

/// The root entry of the layout in the body `data`, of signature
/// `signature`, within `bounds`.
pub(crate) fn layout(
    data: &Data<'_, '_>,
    signature: &Signature,
    bounds: Bounds,
) -> Result<RawEntry, Refused> {
    // A body's signature comes without its outer parentheses and is read
    // back as a structure: compared as signatures, not as text.
    if signature != REPLY {
        return Err(Refused::Malformed);
    }
    let reading = Reading {
        bounds,
        entries: Cell::new(0),
        over: Cell::new(false),
    };
    match data.deserialize_with_seed(ReplySeed(&reading)) {
        Ok((root, _)) => Ok(root),
        Err(_) if reading.over.get() => Err(Refused::OverBound),
        Err(_) => Err(Refused::Malformed),
    }
}

/// A layout being read: its bounds, the entries so far, and whether a
/// bound stopped it.
struct Reading {
    bounds: Bounds,
    entries: Cell<usize>,
    over: Cell<bool>,
}

/// `GetLayout`'s reply: a revision and the root entry.
#[cfg(test)]
pub(crate) fn reply_signature() -> Signature {
    REPLY.clone()
}

impl Reading {
    fn over<E: de::Error>(&self) -> E {
        self.over.set(true);
        E::custom("the layout is larger than its bounds")
    }
}

struct ReplySeed<'r>(&'r Reading);

impl DynamicType for ReplySeed<'_> {
    fn signature(&self) -> Signature {
        REPLY.clone()
    }
}

impl<'de> DeserializeSeed<'de> for ReplySeed<'_> {
    type Value = RawEntry;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawEntry, D::Error> {
        deserializer.deserialize_tuple(2, self)
    }
}

impl<'de> Visitor<'de> for ReplySeed<'_> {
    type Value = RawEntry;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a revision and a layout")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawEntry, A::Error> {
        let _revision: u32 = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        seq.next_element_seed(EntrySeed {
            reading: self.0,
            depth: 0,
        })?
        .ok_or_else(|| de::Error::invalid_length(1, &self))
    }
}

/// An entry `depth` levels below the root (the root's own entries are at
/// depth 1).
#[derive(Clone, Copy)]
struct EntrySeed<'r> {
    reading: &'r Reading,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for EntrySeed<'_> {
    type Value = RawEntry;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawEntry, D::Error> {
        deserializer.deserialize_tuple(3, self)
    }
}

impl<'de> Visitor<'de> for EntrySeed<'_> {
    type Value = RawEntry;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a menu entry")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawEntry, A::Error> {
        let id = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let properties = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(1, &self))?;
        let children = seq
            .next_element_seed(ChildrenSeed(EntrySeed {
                reading: self.reading,
                depth: self.depth + 1,
            }))?
            .ok_or_else(|| de::Error::invalid_length(2, &self))?;
        Ok(RawEntry {
            id,
            properties,
            children,
        })
    }
}

/// An entry's children, `av`: each counted against the bounds before it is
/// read.
struct ChildrenSeed<'r>(EntrySeed<'r>);

impl<'de> DeserializeSeed<'de> for ChildrenSeed<'_> {
    type Value = Vec<RawEntry>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<RawEntry>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for ChildrenSeed<'_> {
    type Value = Vec<RawEntry>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an entry's children")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<RawEntry>, A::Error> {
        let EntrySeed { reading, depth } = self.0;
        let bounds = reading.bounds;
        let mut children = Vec::new();
        loop {
            // Each child is counted before it is decoded: the first one
            // over a bound stops the read.
            if children.len() == bounds.per_level
                || reading.entries.get() == bounds.entries
                || depth > bounds.depth + 1
            {
                return match seq.next_element::<IgnoredAny>()? {
                    Some(_) => Err(reading.over()),
                    None => Ok(children),
                };
            }
            match seq.next_element_seed(VariantSeed(self.0))? {
                Some(child) => children.push(child),
                None => return Ok(children),
            }
        }
    }
}

/// A child: a variant that must hold an entry.
struct VariantSeed<'r>(EntrySeed<'r>);

impl<'de> DeserializeSeed<'de> for VariantSeed<'_> {
    type Value = RawEntry;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawEntry, D::Error> {
        // A variant reads as the pair (signature, value), as zbus's own
        // `as_variant` reads it.
        deserializer.deserialize_struct("Variant", &["signature", "value"], self)
    }
}

impl<'de> Visitor<'de> for VariantSeed<'_> {
    type Value = RawEntry;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a variant holding a menu entry")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawEntry, A::Error> {
        let signature: Signature = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let reading = self.0.reading;
        if signature != *ENTRY {
            return Err(de::Error::custom("a child that is not an entry"));
        }
        reading.entries.set(reading.entries.get() + 1);
        seq.next_element_seed(self.0)?
            .ok_or_else(|| de::Error::invalid_length(1, &self))
    }
}

/// The menu object's own properties this host reads (`GetAll` of
/// `com.canonical.dbusmenu`); `None` for each one absent or of the wrong
/// type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RawMenuProperties {
    pub(crate) text_direction: Option<String>,
    pub(crate) icon_theme_path: Option<Vec<String>>,
}

impl Type for RawMenuProperties {
    const SIGNATURE: &'static Signature = <HashMap<String, OwnedVariant> as Type>::SIGNATURE;
}

impl<'de> serde::Deserialize<'de> for RawMenuProperties {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(MenuPropertiesVisitor)
    }
}

struct MenuPropertiesVisitor;

impl<'de> Visitor<'de> for MenuPropertiesVisitor {
    type Value = RawMenuProperties;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a{sv} of menu properties")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawMenuProperties, A::Error> {
        let mut p = RawMenuProperties::default();
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "TextDirection" => p.text_direction = map.next_value::<Lenient<_>>()?.0,
                "IconThemePath" => p.icon_theme_path = map.next_value::<Lenient<_>>()?.0,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(p)
    }
}

impl<'de> serde::Deserialize<'de> for RawProperties {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(PropertiesVisitor)
    }
}

struct PropertiesVisitor;

impl<'de> Visitor<'de> for PropertiesVisitor {
    type Value = RawProperties;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a{sv} of menu entry properties")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawProperties, A::Error> {
        fn take<'de, A: MapAccess<'de>, T: Type + de::Deserialize<'de>>(
            map: &mut A,
        ) -> Result<Option<T>, A::Error> {
            map.next_value::<Lenient<T>>().map(|v| v.0)
        }
        let mut p = RawProperties::default();
        while let Some(key) = map.next_key::<&str>()? {
            match key {
                "type" => p.kind = take(&mut map)?,
                "label" => p.label = take(&mut map)?,
                "enabled" => p.enabled = take(&mut map)?,
                "visible" => p.visible = take(&mut map)?,
                "icon-name" => p.icon_name = take(&mut map)?,
                "icon-data" => p.icon_data = take(&mut map)?,
                "toggle-type" => p.toggle_type = take(&mut map)?,
                "toggle-state" => p.toggle_state = take(&mut map)?,
                "children-display" => p.children_display = take(&mut map)?,
                "disposition" => p.disposition = take(&mut map)?,
                "shortcut" => p.shortcut = take(&mut map)?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(p)
    }
}
