//! The effective settings, value by value, with the layer each came from:
//! what a "show my configuration" command prints.

use std::fmt;
use std::path::PathBuf;

use crate::diagnostic::Location;
use crate::snapshot::{Contents, Snapshot};
use crate::tree::{Node, Origin};

/// A section's effective values and where each came from.
///
/// Its [`Display`](fmt::Display) is TOML with the source of each value in a
/// comment:
///
/// ```text
/// [panel]
/// edge = "top"  # default
/// height = 40  # /home/u/.config/deskrinch/config.toml:2:10
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    /// The section.
    pub section: String,
    /// Every value that is not a table, in key order. Keys the settings do
    /// not know are left out.
    pub values: Vec<EffectiveValue>,
}

/// One effective value.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveValue {
    /// The key within the section, as TOML addresses it (`clock.format`).
    pub key: String,
    /// The value, as written.
    pub value: toml::Value,
    /// The layer it came from.
    pub source: ValueSource,
}

/// The layer a value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueSource {
    /// The component's built-in defaults.
    Default,
    /// A configuration file.
    File {
        /// The file.
        path: PathBuf,
        /// Where the value is written.
        location: Location,
        /// Whether the value comes from the file's last good version
        /// because the file as it is now is broken; the location is then in
        /// that version.
        kept: bool,
    },
}

impl Effective {
    pub(crate) fn new(section: &str, merged: &Node, snapshot: &Snapshot) -> Self {
        let is_current = |file: &crate::tree::SourceFile| {
            snapshot
                .files
                .iter()
                .any(|current| match &current.contents {
                    Contents::Parsed { file: current, .. } => **current == *file,
                    _ => false,
                })
        };
        let values = merged
            .leaves()
            .into_iter()
            .map(|(key, node)| EffectiveValue {
                key: key.to_string(),
                value: node.to_value(),
                source: match &node.origin {
                    Origin::Default => ValueSource::Default,
                    Origin::File { file, span } => ValueSource::File {
                        path: file.path.clone(),
                        location: Location::of(&file.text, span.start),
                        kept: !is_current(file),
                    },
                },
            })
            .collect();
        Self {
            section: section.to_owned(),
            values,
        }
    }
}

impl fmt::Display for Effective {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "[{}]", self.section)?;
        for value in &self.values {
            writeln!(f, "{value}")?;
        }
        Ok(())
    }
}

impl fmt::Display for EffectiveValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = {}  # {}", self.key, self.value, self.source)
    }
}

impl fmt::Display for ValueSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("default"),
            Self::File {
                path,
                location,
                kept,
            } => {
                write!(f, "{}:{location}", path.display())?;
                if *kept {
                    f.write_str(" (last good version)")?;
                }
                Ok(())
            }
        }
    }
}
