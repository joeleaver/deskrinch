//! An icon theme as its `index.theme` describes it.

use deskrinch_keyfile::{Group, KeyFile, LocalizedString};

use crate::problem::ProblemKind;

/// The group every `index.theme` describes its theme in.
const THEME_GROUP: &str = "Icon Theme";
/// The separator of `Directories`, `ScaledDirectories` and `Inherits`.
const SEPARATOR: char = ',';

/// An icon theme, read from its `index.theme`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// The theme's directory name, which names it in `Inherits` and in a
    /// lookup.
    pub name: String,
    /// `Name`: what a theme chooser shows.
    pub title: Option<LocalizedString>,
    /// `Comment`.
    pub comment: Option<LocalizedString>,
    /// `Inherits`: the themes looked in after this one, in order.
    pub inherits: Vec<String>,
    /// `Directories` and then `ScaledDirectories`, each once, without those
    /// that are listed but unusable (see [`ProblemKind`]).
    pub directories: Vec<Directory>,
    /// `Hidden`: whether a theme chooser leaves the theme out.
    pub hidden: bool,
}

/// One directory of icons in a theme, and the sizes it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    /// The directory, relative to the theme, such as `48x48/apps`.
    pub path: String,
    /// `Size`: the nominal size of its icons, in logical pixels.
    pub size: u32,
    /// `Scale`: the scale its icons are drawn for; 1 by default.
    pub scale: u32,
    /// `Type` and the sizes it implies.
    pub kind: DirectoryKind,
    /// `Context`, such as `Applications`.
    pub context: Option<String>,
}

/// A directory's `Type`: which sizes its icons serve without scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryKind {
    /// Exactly [`Directory::size`].
    Fixed,
    /// Any size from `min` to `max` (`MinSize` and `MaxSize`, each
    /// [`Directory::size`] by default).
    Scalable {
        /// `MinSize`.
        min: u32,
        /// `MaxSize`.
        max: u32,
    },
    /// Within `threshold` (`Threshold`, 2 by default) of
    /// [`Directory::size`]. The default `Type`.
    ///
    /// When no directory matches, a lookup takes the closest, and a
    /// `Threshold` directory's distance here is from the band
    /// `Size ± Threshold` (times `Scale`, in device pixels). The
    /// specification's text compares against that band but then subtracts
    /// `MinSize` and `MaxSize`, which only `Scalable` directories have; this
    /// crate reads the text's evident intent.
    Threshold {
        /// `Threshold`.
        threshold: u32,
    },
}

impl Theme {
    /// Reads the theme `name` from the contents of its `index.theme`,
    /// reporting what is wrong in `problems`. `None` when the file cannot
    /// describe a theme at all: it is not a key file, or has no
    /// `[Icon Theme]` group.
    pub fn parse(name: &str, contents: &[u8], problems: &mut Vec<ProblemKind>) -> Option<Self> {
        let file = match KeyFile::parse(contents, problems) {
            Ok(file) => file,
            Err(error) => {
                problems.push(ProblemKind::Syntax(error));
                return None;
            }
        };
        let Some(main) = file.group(THEME_GROUP) else {
            problems.push(ProblemKind::NotAnIconTheme);
            return None;
        };
        let mut listed = main.list_separated("Directories", SEPARATOR, problems);
        for scaled in main.list_separated("ScaledDirectories", SEPARATOR, problems) {
            if !listed.contains(&scaled) {
                listed.push(scaled);
            }
        }
        let mut directories = Vec::new();
        for path in listed {
            let Some(group) = file.group(&path) else {
                problems.push(ProblemKind::DirectoryWithoutGroup { directory: path });
                continue;
            };
            if let Some(directory) = Directory::parse(path, group, problems) {
                directories.push(directory);
            }
        }
        Some(Self {
            name: name.to_owned(),
            title: main.localized("Name", problems),
            comment: main.localized("Comment", problems),
            inherits: main.list_separated("Inherits", SEPARATOR, problems),
            directories,
            hidden: main.boolean("Hidden", problems),
        })
    }
}

impl Directory {
    fn parse(path: String, group: &Group, problems: &mut Vec<ProblemKind>) -> Option<Self> {
        let positive = |key, problems: &mut Vec<ProblemKind>| {
            group
                .integer(key, problems)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value >= 1)
        };
        let Some(size) = positive("Size", problems) else {
            problems.push(ProblemKind::InvalidDirectory {
                directory: path,
                key: "Size",
            });
            return None;
        };
        let scale = if group.has("Scale") {
            let Some(scale) = positive("Scale", problems) else {
                problems.push(ProblemKind::InvalidDirectory {
                    directory: path,
                    key: "Scale",
                });
                return None;
            };
            scale
        } else {
            1
        };
        let bounded = |key, problems: &mut Vec<ProblemKind>| {
            group
                .integer(key, problems)
                .and_then(|value| u32::try_from(value).ok())
        };
        let kind = match group.string("Type", problems).as_deref() {
            Some("Fixed") => DirectoryKind::Fixed,
            Some("Scalable") => DirectoryKind::Scalable {
                min: bounded("MinSize", problems).unwrap_or(size),
                max: bounded("MaxSize", problems).unwrap_or(size),
            },
            other => {
                if let Some(value) = other.filter(|value| *value != "Threshold") {
                    problems.push(ProblemKind::UnknownType {
                        directory: path.clone(),
                        value: value.to_owned(),
                    });
                }
                DirectoryKind::Threshold {
                    threshold: bounded("Threshold", problems).unwrap_or(2),
                }
            }
        };
        Some(Self {
            context: group.string("Context", problems),
            path,
            size,
            scale,
            kind,
        })
    }

    /// Whether the directory serves `size` at `scale` as it is
    /// (`DirectoryMatchesSize` in the specification).
    pub(crate) fn matches(&self, size: u32, scale: u32) -> bool {
        if self.scale != scale {
            return false;
        }
        let (min, max) = self.range();
        (min..=max).contains(&i64::from(size))
    }

    /// How far the directory's icons are from `size` at `scale`, in
    /// device pixels (`DirectorySizeDistance` in the specification).
    pub(crate) fn distance(&self, size: u32, scale: u32) -> i64 {
        let wanted = i64::from(size) * i64::from(scale);
        let own = i64::from(self.scale);
        let (min, max) = self.range();
        let (min, max) = (min * own, max * own);
        if wanted < min {
            min - wanted
        } else if wanted > max {
            wanted - max
        } else {
            0
        }
    }

    /// The sizes the directory serves, in logical pixels. For `Threshold`
    /// this is `Size ± Threshold`, which is what the specification's
    /// `DirectorySizeDistance` compares against; its text then subtracts
    /// `MinSize` and `MaxSize`, which a `Threshold` directory does not
    /// have (see [`DirectoryKind::Threshold`]).
    fn range(&self) -> (i64, i64) {
        let size = i64::from(self.size);
        match self.kind {
            DirectoryKind::Fixed => (size, size),
            DirectoryKind::Scalable { min, max } => (i64::from(min), i64::from(max)),
            DirectoryKind::Threshold { threshold } => {
                let threshold = i64::from(threshold);
                (size - threshold, size + threshold)
            }
        }
    }
}

#[cfg(test)]
#[path = "theme_tests.rs"]
pub(crate) mod tests;
