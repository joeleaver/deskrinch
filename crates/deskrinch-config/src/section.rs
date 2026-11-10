//! A component's settings: its section of every layer, validated layer by
//! layer against its type, and merged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::diagnostic::{Diagnostic, Problem};
use crate::effective::Effective;
use crate::error::DefaultsError;
use crate::snapshot::{Contents, Snapshot};
use crate::tree::{KeyPath, Node, Segment, SourceFile};

/// What a component's settings type must be: written and read by `serde`,
/// with built-in defaults, comparable (so an unchanged reload delivers
/// nothing) and sendable to the watcher's thread. Every type with those
/// traits is one.
///
/// The defaults are the lowest layer, so a file sets only what it changes.
/// Do not use `#[serde(deny_unknown_fields)]`: an unknown key is reported as
/// a warning, and the rest of the file still applies.
pub trait Settings:
    Serialize + DeserializeOwned + Default + Clone + PartialEq + Send + 'static
{
}

impl<T> Settings for T where
    T: Serialize + DeserializeOwned + Default + Clone + PartialEq + Send + 'static
{
}

/// One component's section (the `[name]` table) of the configuration,
/// resolved from snapshots of the files.
///
/// It remembers each file's last good part of the section: when a file
/// that was good breaks, [`resolve`](Self::resolve) reports why and keeps
/// using the file's last good version, so a half-finished edit does not
/// throw the component back to the layers below. A file broken from the
/// first resolve is skipped.
#[derive(Debug, Clone)]
pub struct Section<T> {
    name: String,
    defaults: Node,
    base: T,
    kept: HashMap<PathBuf, Node>,
}

/// A component's settings, and how they came to be.
#[derive(Debug, Clone)]
pub struct Resolved<T> {
    /// The settings: the defaults with every usable layer laid over them.
    pub settings: T,
    /// Every problem found, lowest layer first: files broken as a whole,
    /// then, file by file, the section's errors and unknown keys.
    pub diagnostics: Vec<Diagnostic>,
    /// Every effective value and the layer it came from.
    pub effective: Effective,
}

impl<T: Settings> Section<T> {
    /// The section `name` (the table `[name]`), with `T::default()` as its
    /// lowest layer.
    ///
    /// # Errors
    ///
    /// [`DefaultsError`] when `T::default()` cannot be written as a TOML
    /// table, or does not read back as a `T`. That is a bug in `T`, the same
    /// on every run, so a component's tests find it.
    pub fn new(name: impl Into<String>) -> Result<Self, DefaultsError> {
        let section = name.into();
        let value =
            toml::Value::try_from(T::default()).map_err(|source| DefaultsError::Serialize {
                section: section.clone(),
                source,
            })?;
        if !value.is_table() {
            return Err(DefaultsError::NotATable { section });
        }
        let defaults = Node::from_default(value);
        let base = match validate::<T>(&defaults) {
            Ok((base, _)) => base,
            Err((_, message)) => return Err(DefaultsError::RoundTrip { section, message }),
        };
        Ok(Self {
            name: section,
            defaults,
            base,
            kept: HashMap::new(),
        })
    }

    /// The section's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The settings `snapshot` gives, layer by layer from the lowest.
    ///
    /// A layer is used when the result still reads as a `T`; otherwise its
    /// error is reported with its location and the layer is replaced by its
    /// file's last good version, if an earlier resolve had one, or skipped.
    /// Keys `T` does not know are reported as warnings where the file that
    /// is in use writes them.
    pub fn resolve(&mut self, snapshot: &Snapshot) -> Resolved<T> {
        let mut state = Layering {
            name: &self.name,
            merged: self.defaults.clone(),
            settings: self.base.clone(),
            ignored: Vec::new(),
            diagnostics: Vec::new(),
        };
        let mut kept = HashMap::new();
        for file in &snapshot.files {
            match &file.contents {
                Contents::Absent => continue,
                Contents::Broken(diagnostic) => state.diagnostics.push(diagnostic.clone()),
                Contents::Parsed { file: source, root } => {
                    let Some(entry) = root.table().and_then(|table| table.get(&self.name)) else {
                        continue;
                    };
                    if entry.value.table().is_none() {
                        let problem = Problem::NotATable {
                            section: self.name.clone(),
                            found: entry.value.type_name(),
                        };
                        let origin = &entry.value.origin;
                        state
                            .diagnostics
                            .push(Diagnostic::at_origin(source, origin, problem));
                    } else if state.apply_fresh(source, &entry.value) {
                        kept.insert(file.path.clone(), entry.value.clone());
                        continue;
                    }
                }
            }
            if let Some(last_good) = self.kept.get(&file.path)
                && state.apply_kept(&file.path, last_good)
            {
                kept.insert(file.path.clone(), last_good.clone());
            }
        }
        self.kept = kept;
        let mut effective = state.merged;
        for path in &state.ignored {
            effective.remove(path);
        }
        Resolved {
            settings: state.settings,
            diagnostics: state.diagnostics,
            effective: Effective::new(&self.name, &effective, snapshot),
        }
    }
}

/// The layers laid so far.
struct Layering<'a, T> {
    name: &'a str,
    merged: Node,
    settings: T,
    /// The keys `settings` ignored.
    ignored: Vec<KeyPath>,
    diagnostics: Vec<Diagnostic>,
}

impl<T: Settings> Layering<'_, T> {
    /// Lays the section as `source` has it now, reporting its unknown keys;
    /// or reports why it cannot be laid.
    fn apply_fresh(&mut self, source: &SourceFile, layer: &Node) -> bool {
        match self.try_apply(layer) {
            Ok(()) => {
                let mut unknown: Vec<Diagnostic> = self
                    .ignored
                    .iter()
                    .filter(|path| layer.contains(path))
                    .map(|path| {
                        let problem = Problem::UnknownKey {
                            section: self.name.to_owned(),
                            key: path.to_string(),
                        };
                        Diagnostic::at_origin(source, layer.locate(path).0, problem)
                    })
                    .collect();
                // In the order the file has them.
                unknown.sort_by_key(|diagnostic| diagnostic.location);
                self.diagnostics.append(&mut unknown);
                true
            }
            Err((path, message)) => {
                let origin = layer.value_origin(&path);
                let problem = self.invalid(&path, message);
                self.diagnostics
                    .push(Diagnostic::at_origin(source, origin, problem));
                false
            }
        }
    }

    /// Lays a file's last good version of the section in place of its
    /// broken current one. Its unknown keys were reported when it was
    /// current. If it no longer fits the layers below, the error has no
    /// location: the version it refers to is gone.
    fn apply_kept(&mut self, path: &Path, layer: &Node) -> bool {
        match self.try_apply(layer) {
            Ok(()) => true,
            Err((key, message)) => {
                let problem = self.invalid(&key, message);
                self.diagnostics.push(Diagnostic {
                    file: path.to_path_buf(),
                    location: None,
                    problem,
                });
                false
            }
        }
    }

    fn try_apply(&mut self, layer: &Node) -> Result<(), (KeyPath, String)> {
        let mut trial = self.merged.clone();
        trial.merge(layer.clone());
        let (settings, ignored) = validate::<T>(&trial)?;
        self.merged = trial;
        self.settings = settings;
        self.ignored = ignored;
        Ok(())
    }

    fn invalid(&self, path: &KeyPath, message: String) -> Problem {
        Problem::Invalid {
            section: self.name.to_owned(),
            key: (!path.0.is_empty()).then(|| path.to_string()),
            message,
        }
    }
}

/// Reads `node` as a `T`, with the keys `T` ignored; or the path to the
/// value that failed, and why.
fn validate<T: DeserializeOwned>(node: &Node) -> Result<(T, Vec<KeyPath>), (KeyPath, String)> {
    let mut ignored = Vec::new();
    let mut record = |path: serde_ignored::Path<'_>| ignored.push(ignored_path(&path));
    let deserializer = serde_ignored::Deserializer::new(node.to_value(), &mut record);
    match serde_path_to_error::deserialize::<_, T>(deserializer) {
        Ok(settings) => Ok((settings, ignored)),
        Err(error) => {
            let mut path = KeyPath::default();
            for segment in error.path() {
                path.0.push(match segment {
                    serde_path_to_error::Segment::Seq { index } => Segment::Index(*index),
                    serde_path_to_error::Segment::Map { key } => Segment::Key(key.clone()),
                    serde_path_to_error::Segment::Enum { variant } => Segment::Key(variant.clone()),
                    serde_path_to_error::Segment::Unknown => break,
                });
            }
            Err((path, error.into_inner().message().trim_end().to_owned()))
        }
    }
}

fn ignored_path(path: &serde_ignored::Path<'_>) -> KeyPath {
    match path {
        serde_ignored::Path::Root => KeyPath::default(),
        serde_ignored::Path::Seq { parent, index } => {
            let mut path = ignored_path(parent);
            path.0.push(Segment::Index(*index));
            path
        }
        serde_ignored::Path::Map { parent, key } => {
            let mut path = ignored_path(parent);
            path.0.push(Segment::Key(key.clone()));
            path
        }
        serde_ignored::Path::Some { parent }
        | serde_ignored::Path::NewtypeStruct { parent }
        | serde_ignored::Path::NewtypeVariant { parent } => ignored_path(parent),
    }
}
