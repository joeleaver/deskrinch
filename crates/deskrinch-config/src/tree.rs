//! The configuration as a tree of values that remember where they came
//! from, and the layering rule: tables merge key by key, anything else is
//! replaced.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize as _;
use toml::Spanned;
use toml::de::{DeTable, DeValue, ValueDeserializer};

/// One version of a configuration file's text, shared by every value
/// parsed from it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SourceFile {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
}

/// Where a value came from.
#[derive(Debug, Clone)]
pub(crate) enum Origin {
    /// The component's built-in defaults.
    Default,
    /// Bytes `span` of a file.
    File {
        file: Arc<SourceFile>,
        span: Range<usize>,
    },
}

/// A value and where it came from.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) origin: Origin,
    pub(crate) kind: Kind,
}

#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Table(BTreeMap<String, Entry>),
    /// An array is replaced as a whole; its elements keep their own origins
    /// for locating an error inside one.
    Array(Vec<Node>),
    /// Any other value.
    Scalar(toml::Value),
}

/// A key of a table: where the key itself was written, and its value.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) key: Origin,
    pub(crate) value: Node,
}

/// One step of a path into the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    Key(String),
    Index(usize),
}

/// A path into a section, displayed as TOML would address it
/// (`clock.format`, `widgets[2]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeyPath(pub(crate) Vec<Segment>);

impl fmt::Display for KeyPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            match segment {
                Segment::Key(key) if i == 0 => f.write_str(key)?,
                Segment::Key(key) => write!(f, ".{key}")?,
                Segment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

impl Node {
    /// A whole parsed document.
    ///
    /// # Errors
    ///
    /// A value the parser accepted that is not a valid TOML value, such as
    /// an integer that does not fit in 64 bits.
    pub(crate) fn from_document(
        file: &Arc<SourceFile>,
        root: Spanned<DeTable<'_>>,
    ) -> Result<Self, toml::de::Error> {
        let span = root.span();
        Self::from_de(file, Spanned::new(span, DeValue::Table(root.into_inner())))
    }

    fn from_de(
        file: &Arc<SourceFile>,
        value: Spanned<DeValue<'_>>,
    ) -> Result<Self, toml::de::Error> {
        let span = value.span();
        let kind = match value.into_inner() {
            DeValue::Table(table) => Kind::Table(
                table
                    .into_iter()
                    .map(|(key, value)| {
                        let entry = Entry {
                            key: Origin::File {
                                file: Arc::clone(file),
                                span: key.span(),
                            },
                            value: Self::from_de(file, value)?,
                        };
                        Ok((key.into_inner().into_owned(), entry))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            DeValue::Array(array) => Kind::Array(
                array
                    .into_iter()
                    .map(|element| Self::from_de(file, element))
                    .collect::<Result<_, _>>()?,
            ),
            scalar => Kind::Scalar(toml::Value::deserialize(ValueDeserializer::from(
                Spanned::new(span.clone(), scalar),
            ))?),
        };
        Ok(Self {
            origin: Origin::File {
                file: Arc::clone(file),
                span,
            },
            kind,
        })
    }

    /// A value from the built-in defaults.
    pub(crate) fn from_default(value: toml::Value) -> Self {
        let kind = match value {
            toml::Value::Table(table) => Kind::Table(
                table
                    .into_iter()
                    .map(|(key, value)| {
                        let entry = Entry {
                            key: Origin::Default,
                            value: Self::from_default(value),
                        };
                        (key, entry)
                    })
                    .collect(),
            ),
            toml::Value::Array(array) => {
                Kind::Array(array.into_iter().map(Self::from_default).collect())
            }
            scalar => Kind::Scalar(scalar),
        };
        Self {
            origin: Origin::Default,
            kind,
        }
    }

    /// The entries, if this is a table.
    pub(crate) fn table(&self) -> Option<&BTreeMap<String, Entry>> {
        match &self.kind {
            Kind::Table(table) => Some(table),
            _ => None,
        }
    }

    /// The TOML type name of the value, for messages.
    pub(crate) fn type_name(&self) -> &'static str {
        match &self.kind {
            Kind::Table(_) => "table",
            Kind::Array(_) => "array",
            Kind::Scalar(value) => value.type_str(),
        }
    }

    /// Lays `upper` over `self`: tables merge key by key, anything else is
    /// replaced.
    pub(crate) fn merge(&mut self, upper: Self) {
        match (&mut self.kind, upper.kind) {
            (Kind::Table(lower), Kind::Table(upper_table)) => {
                for (key, entry) in upper_table {
                    match lower.get_mut(&key) {
                        Some(existing) => {
                            existing.key = entry.key;
                            existing.value.merge(entry.value);
                        }
                        None => {
                            lower.insert(key, entry);
                        }
                    }
                }
                self.origin = upper.origin;
            }
            (_, kind) => {
                *self = Self {
                    origin: upper.origin,
                    kind,
                };
            }
        }
    }

    /// The plain TOML value, for deserializing.
    pub(crate) fn to_value(&self) -> toml::Value {
        match &self.kind {
            Kind::Table(table) => toml::Value::Table(
                table
                    .iter()
                    .map(|(key, entry)| (key.clone(), entry.value.to_value()))
                    .collect(),
            ),
            Kind::Array(array) => toml::Value::Array(array.iter().map(Self::to_value).collect()),
            Kind::Scalar(value) => value.clone(),
        }
    }

    /// Whether the tree has a value at `path`.
    pub(crate) fn contains(&self, path: &KeyPath) -> bool {
        self.locate(path).1 == path.0.len()
    }

    /// Where `path` was written in this tree: the origin of the key at its
    /// end when `key` is asked for, else of its value; for a path that
    /// leaves the tree, the deepest point it reaches. Also returns how many
    /// segments were followed.
    pub(crate) fn locate(&self, path: &KeyPath) -> (&Origin, usize) {
        let mut node = self;
        let mut key_origin = None;
        for (depth, segment) in path.0.iter().enumerate() {
            let next = match (&node.kind, segment) {
                (Kind::Table(table), Segment::Key(key)) => table.get(key).map(|entry| {
                    key_origin = Some(&entry.key);
                    &entry.value
                }),
                (Kind::Array(array), Segment::Index(index)) => {
                    key_origin = None;
                    array.get(*index)
                }
                _ => None,
            };
            match next {
                Some(next) => node = next,
                None => return (&node.origin, depth),
            }
        }
        (key_origin.unwrap_or(&node.origin), path.0.len())
    }

    /// The origin of the value at `path`, or of the deepest point it reaches.
    pub(crate) fn value_origin(&self, path: &KeyPath) -> &Origin {
        let mut node = self;
        for segment in &path.0 {
            let next = match (&node.kind, segment) {
                (Kind::Table(table), Segment::Key(key)) => table.get(key).map(|e| &e.value),
                (Kind::Array(array), Segment::Index(index)) => array.get(*index),
                _ => None,
            };
            match next {
                Some(next) => node = next,
                None => break,
            }
        }
        &node.origin
    }

    /// Removes the value at `path`, if there is one.
    pub(crate) fn remove(&mut self, path: &KeyPath) {
        let Some((last, parents)) = path.0.split_last() else {
            return;
        };
        let mut node = self;
        for segment in parents {
            let next = match (&mut node.kind, segment) {
                (Kind::Table(table), Segment::Key(key)) => table.get_mut(key).map(|e| &mut e.value),
                (Kind::Array(array), Segment::Index(index)) => array.get_mut(*index),
                _ => None,
            };
            match next {
                Some(next) => node = next,
                None => return,
            }
        }
        if let (Kind::Table(table), Segment::Key(key)) = (&mut node.kind, last) {
            table.remove(key);
        }
    }

    /// Every value that is not a table, depth first in key order, with its
    /// path.
    pub(crate) fn leaves(&self) -> Vec<(KeyPath, &Self)> {
        let mut leaves = Vec::new();
        self.collect_leaves(&mut KeyPath::default(), &mut leaves);
        leaves
    }

    fn collect_leaves<'a>(&'a self, path: &mut KeyPath, leaves: &mut Vec<(KeyPath, &'a Self)>) {
        match &self.kind {
            Kind::Table(table) => {
                for (key, entry) in table {
                    path.0.push(Segment::Key(key.clone()));
                    entry.value.collect_leaves(path, leaves);
                    path.0.pop();
                }
            }
            _ => leaves.push((path.clone(), self)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn parse(text: &str) -> Node {
        let file = Arc::new(SourceFile {
            path: "/test.toml".into(),
            text: text.to_owned(),
        });
        let root = DeTable::parse(text).unwrap();
        Node::from_document(&file, root).unwrap()
    }

    fn key(path: &str) -> KeyPath {
        KeyPath(path.split('.').map(|k| Segment::Key(k.into())).collect())
    }

    fn span(origin: &Origin) -> Range<usize> {
        match origin {
            Origin::File { span, .. } => span.clone(),
            Origin::Default => panic!("from the defaults"),
        }
    }

    #[test]
    fn merges_tables_and_replaces_everything_else() {
        let mut lower = parse("a = 1\nlist = [1, 2]\n[t]\nx = 1\ny = 2\n[s]\nz = 1\n");
        lower.merge(parse("list = [3]\ns = 5\n[t]\ny = 20\nw = 4\n"));
        let value = lower.to_value();
        let expected: toml::Value =
            toml::from_str("a = 1\nlist = [3]\ns = 5\n[t]\nx = 1\ny = 20\nw = 4\n").unwrap();
        assert_eq!(value, expected);
    }

    #[test]
    fn locates_keys_values_and_the_deepest_point_reached() {
        let text = "[t]\nx = 'v'\nlist = [1, 'two']\n";
        let node = parse(text);
        let (origin, depth) = node.locate(&key("t.x"));
        assert_eq!((&text[span(origin)], depth), ("x", 2));
        assert_eq!(&text[span(node.value_origin(&key("t.x")))], "'v'");
        let mut element = key("t.list");
        element.0.push(Segment::Index(1));
        assert_eq!(&text[span(node.locate(&element).0)], "'two'");
        assert_eq!(node.locate(&key("t.missing")).1, 1);
        assert!(node.contains(&key("t.x")));
        assert!(!node.contains(&key("t.x.deeper")));
    }

    #[test]
    fn removes_and_lists_leaves() {
        let mut node = parse("[t]\nx = 1\n[t.u]\ny = 2\n");
        node.remove(&key("t.x"));
        let paths: Vec<String> = node.leaves().iter().map(|(p, _)| p.to_string()).collect();
        assert_eq!(paths, ["t.u.y"]);
        let mut path = key("a");
        path.0.push(Segment::Index(2));
        path.0.push(Segment::Key("b".into()));
        assert_eq!(path.to_string(), "a[2].b");
    }
}
