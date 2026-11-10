//! Decoding the `a{sv}` hints straight into [`Hints`].
//!
//! Each standard hint is read as its specified type, borrowing strings and
//! pixel data from the message, so a large `image-data` costs no copy before
//! it is validated. A hint of the wrong type, and any hint the specification
//! does not define, is skipped without being decoded: the specification says
//! to ignore hints a server does not understand. One leniency: `urgency` is
//! a byte, but some senders (`gdbus call`, scripting bindings) send it as
//! another integer type, which is accepted when the level fits a byte.

use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{
    self, Deserialize, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
use zbus::{OwnedVariant, Signature, Type};

use crate::image::RawImage;
use crate::request::Hints;

impl Type for Hints<'_> {
    const SIGNATURE: &'static Signature = <HashMap<String, OwnedVariant>>::SIGNATURE;
}

impl<'de> Deserialize<'de> for Hints<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(HintsVisitor)
    }
}

struct HintsVisitor;

impl<'de> Visitor<'de> for HintsVisitor {
    type Value = Hints<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a dictionary of hints")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut hints = Hints::default();
        while let Some(key) = map.next_key::<&'de str>()? {
            match key {
                "urgency" => {
                    hints.urgency = map.next_value_seed(Typed::<Urgency>::new())?.map(|u| u.0);
                }
                "category" => hints.category = map.next_value_seed(Typed::new())?,
                "desktop-entry" => hints.desktop_entry = map.next_value_seed(Typed::new())?,
                "image-data" => hints.image_data = image(&mut map)?,
                "image_data" => hints.image_data_legacy = image(&mut map)?,
                "image-path" => hints.image_path = map.next_value_seed(Typed::new())?,
                "image_path" => hints.image_path_legacy = map.next_value_seed(Typed::new())?,
                "icon_data" => hints.icon_data = image(&mut map)?,
                "sound-file" => hints.sound_file = map.next_value_seed(Typed::new())?,
                "sound-name" => hints.sound_name = map.next_value_seed(Typed::new())?,
                "suppress-sound" => hints.suppress_sound = map.next_value_seed(Typed::new())?,
                "transient" => hints.transient = map.next_value_seed(Typed::new())?,
                "resident" => hints.resident = map.next_value_seed(Typed::new())?,
                "action-icons" => hints.action_icons = map.next_value_seed(Typed::new())?,
                "x" => hints.x = map.next_value_seed(Typed::new())?,
                "y" => hints.y = map.next_value_seed(Typed::new())?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(hints)
    }
}

/// The `(iiibiiay)` structure, as the specification lays it out.
type ImageTuple<'a> = (i32, i32, i32, bool, i32, i32, &'a [u8]);

fn image<'de, A: MapAccess<'de>>(map: &mut A) -> Result<Option<RawImage<'de>>, A::Error> {
    let tuple = map.next_value_seed(Typed::<ImageTuple<'de>>::new())?;
    Ok(tuple.map(
        |(width, height, rowstride, has_alpha, bits_per_sample, channels, data)| RawImage {
            width,
            height,
            rowstride,
            has_alpha,
            bits_per_sample,
            channels,
            data,
        },
    ))
}

/// Reads a variant's value as `T` when the variant holds a `T`, and skips it
/// (yielding `None`) when it holds anything else.
struct Typed<T>(PhantomData<T>);

impl<T> Typed<T> {
    fn new() -> Self {
        Self(PhantomData)
    }
}

/// A value [`Typed`] can read: its D-Bus type, or the types it accepts.
trait Accepted<'de>: Sized {
    /// Reads the value if `signature` is acceptable; `Ok(None)` when it is
    /// not, after skipping the value.
    fn read<A: SeqAccess<'de>>(
        signature: &Signature,
        seq: &mut A,
    ) -> Result<Option<Self>, A::Error>;
}

impl<'de, T: Type + Deserialize<'de>> Accepted<'de> for T {
    fn read<A: SeqAccess<'de>>(
        signature: &Signature,
        seq: &mut A,
    ) -> Result<Option<Self>, A::Error> {
        if signature == T::SIGNATURE {
            seq.next_element()
        } else {
            seq.next_element::<IgnoredAny>().map(|_| None)
        }
    }
}

impl<'de, T: Accepted<'de>> DeserializeSeed<'de> for Typed<T> {
    type Value = Option<T>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        // zbus hands a variant over as a two-element structure: its signature,
        // then its value.
        deserializer.deserialize_struct("Variant", &["signature", "value"], self)
    }
}

impl<'de, T: Accepted<'de>> Visitor<'de> for Typed<T> {
    type Value = Option<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a variant")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let signature: Signature = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        T::read(&signature, &mut seq)
    }
}

/// The `urgency` level, read from any integer type.
struct Urgency(u8);

impl<'de> Accepted<'de> for Urgency {
    fn read<A: SeqAccess<'de>>(
        signature: &Signature,
        seq: &mut A,
    ) -> Result<Option<Self>, A::Error> {
        let level = match signature {
            Signature::U8 => seq.next_element::<u8>()?.map(u64::from),
            Signature::U16 => seq.next_element::<u16>()?.map(u64::from),
            Signature::U32 => seq.next_element::<u32>()?.map(u64::from),
            Signature::U64 => seq.next_element::<u64>()?,
            Signature::I16 => seq
                .next_element::<i16>()?
                .and_then(|v| u64::try_from(v).ok()),
            Signature::I32 => seq
                .next_element::<i32>()?
                .and_then(|v| u64::try_from(v).ok()),
            Signature::I64 => seq
                .next_element::<i64>()?
                .and_then(|v| u64::try_from(v).ok()),
            _ => {
                seq.next_element::<IgnoredAny>()?;
                None
            }
        };
        Ok(level.and_then(|l| u8::try_from(l).ok()).map(Self))
    }
}

#[cfg(test)]
mod props;
#[cfg(test)]
mod tests;
