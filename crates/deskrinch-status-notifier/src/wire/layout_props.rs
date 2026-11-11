//! Reading an untrusted `GetLayout` reply: its bounds, and properties of
//! any layout.

use std::collections::HashMap;

use proptest::prelude::*;
use zbus::wire::serialized::{Context, Data};
use zbus::wire::{LE, Structure, StructureBuilder, Variant};

use super::layout::*;
use deskrinch_untrusted::fuzz::config;

/// A layout to send: an entry, its properties and its children.
#[derive(Debug, Clone, Default)]
struct Sent {
    id: i32,
    label: Option<String>,
    children: Vec<Sent>,
}

impl Sent {
    fn leaf(id: i32, label: &str) -> Self {
        Self {
            id,
            label: Some(label.to_owned()),
            children: Vec::new(),
        }
    }

    fn with(id: i32, children: Vec<Self>) -> Self {
        Self {
            id,
            label: None,
            children,
        }
    }

    fn structure(&self) -> Structure<'static> {
        let mut properties: HashMap<String, Variant<'static>> = HashMap::new();
        if let Some(label) = &self.label {
            properties.insert("label".into(), Variant::from(label.clone()));
        }
        let children: Vec<Variant<'static>> = self
            .children
            .iter()
            .map(|c| Variant::from(c.structure()))
            .collect();
        StructureBuilder::new()
            .add_field(self.id)
            .add_field(properties)
            .add_field(children)
            .build()
            .unwrap()
    }

    /// The raw entry it should read as.
    fn raw(&self) -> RawEntry {
        RawEntry {
            id: self.id,
            properties: RawProperties {
                label: self.label.clone(),
                ..RawProperties::default()
            },
            children: self.children.iter().map(Self::raw).collect(),
        }
    }

    fn count(&self) -> usize {
        self.children.iter().map(|c| 1 + c.count()).sum()
    }
}

fn reply(root: &Sent) -> Data<'static, 'static> {
    let reply = StructureBuilder::new()
        .add_field(7_u32)
        .add_field(root.structure())
        .build()
        .unwrap();
    zbus::wire::to_bytes(Context::new(LE, 0), &reply).unwrap()
}

fn read(data: &Data<'_, '_>, bounds: Bounds) -> Result<RawEntry, Refused> {
    layout(data, &reply_signature(), bounds)
}

const WIDE: Bounds = Bounds {
    depth: 8,
    entries: 1000,
    per_level: 100,
};

/// `depth` levels of one entry each below the root's own entry.
fn deep(depth: usize) -> Sent {
    (0..=depth).fold(Sent::leaf(99, "deepest"), |inner, level| {
        Sent::with(i32::try_from(level).unwrap() + 1, vec![inner])
    })
}

#[test]
fn a_layout_reads_with_its_properties_and_children() {
    let root = Sent::with(
        0,
        vec![
            Sent::leaf(1, "_Open"),
            Sent::with(2, vec![Sent::leaf(3, "Inner")]),
        ],
    );
    assert_eq!(read(&reply(&root), WIDE), Ok(root.raw()));
}

#[test]
fn properties_of_the_wrong_type_or_unknown_read_as_absent() {
    let mut properties: HashMap<&str, Variant<'_>> = HashMap::new();
    properties.insert("label", Variant::from(3_u32));
    properties.insert("enabled", Variant::from(false));
    properties.insert("toggle-state", Variant::from(1_i32));
    properties.insert("x-unknown", Variant::from("anything"));
    properties.insert("shortcut", Variant::from(vec![vec!["Control", "q"]]));
    let entry = StructureBuilder::new()
        .add_field(1_i32)
        .add_field(properties)
        .add_field(Vec::<Variant<'_>>::new())
        .build()
        .unwrap();
    let root = StructureBuilder::new()
        .add_field(0_i32)
        .add_field(HashMap::<&str, Variant<'_>>::new())
        .add_field(vec![Variant::from(entry)])
        .build()
        .unwrap();
    let reply = StructureBuilder::new()
        .add_field(1_u32)
        .add_field(root)
        .build()
        .unwrap();
    let data = zbus::wire::to_bytes(Context::new(LE, 0), &reply).unwrap();
    let entry = &read(&data, WIDE).unwrap().children[0];
    assert_eq!(
        entry.properties,
        RawProperties {
            enabled: Some(false),
            toggle_state: Some(1),
            shortcut: Some(vec![vec!["Control".into(), "q".into()]]),
            ..RawProperties::default()
        }
    );
}

#[test]
fn depth_is_bounded() {
    let bounds = Bounds { depth: 3, ..WIDE };
    // The root's entries, and three levels of submenus below them.
    assert!(read(&reply(&deep(3)), bounds).is_ok());
    assert_eq!(read(&reply(&deep(4)), bounds), Err(Refused::OverBound));
}

#[test]
fn entries_are_bounded_in_all_and_per_level() {
    let level = |n: i32| Sent::with(0, (1..=n).map(|id| Sent::leaf(id, "x")).collect());
    let per_level = Bounds {
        per_level: 4,
        ..WIDE
    };
    assert!(read(&reply(&level(4)), per_level).is_ok());
    assert_eq!(read(&reply(&level(5)), per_level), Err(Refused::OverBound));
    // Two submenus of three: eight entries in all.
    let nested = Sent::with(
        0,
        vec![
            Sent::with(1, (10..13).map(|id| Sent::leaf(id, "a")).collect()),
            Sent::with(2, (20..23).map(|id| Sent::leaf(id, "b")).collect()),
        ],
    );
    assert_eq!(nested.count(), 8);
    let total = |entries| Bounds { entries, ..WIDE };
    assert!(read(&reply(&nested), total(8)).is_ok());
    assert_eq!(read(&reply(&nested), total(7)), Err(Refused::OverBound));
}

#[test]
fn anything_but_a_layout_is_malformed() {
    let root = Sent::with(0, vec![Sent::leaf(1, "a")]);
    let data = reply(&root);
    let other = zbus::wire::Signature::try_from("(us)").unwrap();
    assert_eq!(layout(&data, &other, WIDE), Err(Refused::Malformed));
    // A child that is a variant of something else.
    let root = StructureBuilder::new()
        .add_field(0_i32)
        .add_field(HashMap::<&str, Variant<'_>>::new())
        .add_field(vec![Variant::from("not an entry")])
        .build()
        .unwrap();
    let reply = StructureBuilder::new()
        .add_field(1_u32)
        .add_field(root)
        .build()
        .unwrap();
    let data = zbus::wire::to_bytes(Context::new(LE, 0), &reply).unwrap();
    assert_eq!(read(&data, WIDE), Err(Refused::Malformed));
}

/// A label as the bus carries one: any text but NUL, which no D-Bus string
/// may hold (the bus refuses the message).
const LABEL: &str = "[^\0]{0,12}";

fn tree() -> impl Strategy<Value = Sent> {
    let leaf = (any::<i32>(), proptest::option::of(LABEL)).prop_map(|(id, label)| Sent {
        id,
        label,
        children: Vec::new(),
    });
    leaf.prop_recursive(5, 64, 6, |inner| {
        (
            any::<i32>(),
            proptest::option::of(LABEL),
            prop::collection::vec(inner, 0..6),
        )
            .prop_map(|(id, label, children)| Sent {
                id,
                label,
                children,
            })
    })
}

fn depth(sent: &Sent) -> usize {
    sent.children
        .iter()
        .map(|c| 1 + depth(c))
        .max()
        .unwrap_or(0)
}

fn widest(sent: &Sent) -> usize {
    sent.children
        .iter()
        .map(widest)
        .max()
        .unwrap_or(0)
        .max(sent.children.len())
}

proptest! {
    #![proptest_config(config())]

    /// Any layout reads exactly as it was sent when it is within its
    /// bounds, and is refused as over them otherwise.
    #[test]
    fn any_layout_reads_exactly_or_is_over_its_bounds(
        root in tree(),
        depth_bound in 0_usize..5,
        entries in 0_usize..64,
        per_level in 0_usize..6,
    ) {
        let bounds = Bounds { depth: depth_bound, entries, per_level };
        let within = depth(&root) <= depth_bound + 1
            && root.count() <= entries
            && widest(&root) <= per_level;
        let read = read(&reply(&root), bounds);
        if within {
            prop_assert_eq!(read, Ok(root.raw()));
        } else {
            prop_assert_eq!(read, Err(Refused::OverBound));
        }
    }

    /// Arbitrary bytes are refused or read, never a panic.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let data = Data::new(bytes, Context::new(LE, 0));
        let _outcome = read(&data, WIDE);
    }

    /// A layout cut short or with a byte changed is refused or read within
    /// its bounds, never a panic.
    #[test]
    fn a_damaged_layout_never_panics_or_passes_its_bounds(
        root in tree(),
        at in any::<prop::sample::Index>(),
        value in any::<u8>(),
        end in any::<prop::sample::Index>(),
    ) {
        let mut bytes = reply(&root).to_vec();
        let at = at.index(bytes.len());
        bytes[at] = value;
        bytes.truncate(end.index(bytes.len()) + 1);
        let bounds = Bounds { depth: 2, entries: 16, per_level: 4 };
        if let Ok(read) = read(&Data::new(bytes, Context::new(LE, 0)), bounds) {
            let count = |e: &RawEntry| -> usize {
                fn count(e: &RawEntry) -> usize {
                    e.children.iter().map(|c| 1 + count(c)).sum()
                }
                count(e)
            };
            prop_assert!(count(&read) <= 16);
        }
    }
}
