//! Every file that could answer a request, best first: the order the
//! specification's `FindBestIcon` would take them in, continued past the
//! first.

use std::path::PathBuf;

use crate::index::{Format, Index, Indexed};
use crate::lookup::{HICOLOR, Icon};
use crate::table::Location;

/// The files that could answer a request, best first, as
/// [`Index::candidates`] gives them. Reading them reads no file.
///
/// For each theme consulted (the theme, its parents depth first, each
/// once, then `hicolor`) and, within it, each name tried: the files whose
/// directory serves the size and scale as they are, in the theme's
/// directory order, then base directory, then format; then its other files
/// by distance from the size in device pixels, the first of equal distance
/// first. After every theme, the icons directly in the base directories,
/// for each name in turn, by base directory.
#[derive(Debug)]
pub struct Candidates<'a> {
    index: &'a Index,
    names: Vec<String>,
    size: u32,
    scale: u32,
    themes: Vec<&'a Indexed>,
    stage: Stage,
    /// The current stage's locations, the next one last.
    pending: Vec<Location>,
}

/// Which theme and name, or which unthemed name, `pending` is for.
#[derive(Debug, Clone, Copy)]
enum Stage {
    Themed { theme: usize, name: usize },
    Unthemed { name: usize },
}

impl<'a> Candidates<'a> {
    pub(crate) fn new(index: &'a Index, names: Vec<String>, size: u32, scale: u32) -> Self {
        let mut themes = Vec::new();
        let mut visited = Vec::new();
        for start in [index.theme.as_str(), HICOLOR] {
            in_order(index, start, &mut visited, &mut themes);
        }
        let mut candidates = Self {
            index,
            names,
            size,
            scale,
            themes,
            stage: Stage::Themed { theme: 0, name: 0 },
            pending: Vec::new(),
        };
        candidates.fill();
        candidates
    }

    /// Loads the current stage's locations into `pending`.
    fn fill(&mut self) {
        let mut locations = match self.stage {
            Stage::Themed { theme, name } => {
                let Some(theme) = self.themes.get(theme) else {
                    return;
                };
                let found = theme.icons.get(&self.names[name]);
                let directory = |at: &Location| &theme.theme.directories[at.directory()];
                let (size, scale) = (self.size, self.scale);
                let (mut exact, mut other): (Vec<Location>, Vec<Location>) = found
                    .iter()
                    .partition(|at| directory(at).matches(size, scale));
                // A stable sort keeps the first of equal distance first.
                other.sort_by_key(|at| directory(at).distance(size, scale));
                exact.append(&mut other);
                exact
            }
            Stage::Unthemed { name } => self.index.unthemed.get(&self.names[name]).to_vec(),
        };
        locations.reverse();
        self.pending = locations;
    }

    /// Moves to the next stage; `false` after the last.
    fn advance(&mut self) -> bool {
        let last_name = self.names.len() - 1;
        self.stage = match self.stage {
            Stage::Themed { theme, name } if name < last_name => Stage::Themed {
                theme,
                name: name + 1,
            },
            Stage::Themed { theme, .. } if theme + 1 < self.themes.len() => Stage::Themed {
                theme: theme + 1,
                name: 0,
            },
            Stage::Themed { .. } => Stage::Unthemed { name: 0 },
            Stage::Unthemed { name } if name < last_name => Stage::Unthemed { name: name + 1 },
            Stage::Unthemed { .. } => return false,
        };
        self.fill();
        true
    }

    fn icon(&self, at: Location) -> Icon {
        let path: PathBuf = match self.stage {
            Stage::Themed { theme, name } => {
                self.index
                    .themed_path(self.themes[theme], &self.names[name], at)
            }
            Stage::Unthemed { name } => self.index.unthemed_path(&self.names[name], at),
        };
        Icon {
            path,
            format: at.format(),
        }
    }
}

impl Iterator for Candidates<'_> {
    type Item = Icon;

    fn next(&mut self) -> Option<Icon> {
        loop {
            if let Some(at) = self.pending.pop() {
                return Some(self.icon(at));
            }
            if !self.advance() {
                return None;
            }
        }
    }
}

/// `theme` and its parents, depth first, each once, as far as they are
/// installed.
fn in_order<'a>(
    index: &'a Index,
    theme: &'a str,
    visited: &mut Vec<&'a str>,
    themes: &mut Vec<&'a Indexed>,
) {
    if visited.contains(&theme) {
        return;
    }
    visited.push(theme);
    let Some(indexed) = index.themes.get(theme) else {
        return;
    };
    themes.push(indexed);
    for parent in &indexed.theme.inherits {
        in_order(index, parent, visited, themes);
    }
}

/// The names a request tries, in order. With the generic fallback, each
/// name with its last `-` component dropped, as the Icon Naming
/// Specification describes; a symbolic name (`a-b-symbolic`) tries the
/// symbolic forms first (`a-b-symbolic`, `a-symbolic`), then the coloured
/// ones (`a-b`, `a`): a monochrome icon was asked for.
pub(crate) fn names(name: &str, generic: bool) -> Vec<String> {
    if !generic {
        return vec![name.to_owned()];
    }
    match name
        .strip_suffix("-symbolic")
        .filter(|base| !base.is_empty())
    {
        Some(base) => {
            let coloured = shortened(base);
            coloured
                .iter()
                .map(|name| format!("{name}-symbolic"))
                .chain(coloured.iter().map(|name| (*name).to_owned()))
                .collect()
        }
        None => shortened(name).into_iter().map(str::to_owned).collect(),
    }
}

/// `name`, then each name with its last `-` component dropped.
fn shortened(name: &str) -> Vec<&str> {
    let mut names = vec![name];
    let mut rest = name;
    while let Some(end) = rest.rfind('-') {
        rest = &rest[..end];
        if rest.is_empty() {
            break;
        }
        names.push(rest);
    }
    names
}

/// The format of an absolute path's file, by its extension: one of the
/// specification's three, or [`Format::Other`].
pub(crate) fn format_of(path: &std::path::Path) -> Format {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(Format::split)
        .map_or(Format::Other, |(_, format)| format)
}
