//! Building an [`Index`] from the base directories: the only part of the
//! crate that reads files.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use deskrinch_xdg::BaseDirs;

use crate::builder::TableBuilder;
use crate::index::{Index, Indexed};
use crate::listing::Lister;
use crate::lookup::HICOLOR;
use crate::problem::{Problem, ProblemKind};
use crate::table::NameTable;
use crate::theme::Theme;

/// Where unthemed icons are by the specification, after the base
/// directories that hold themes.
const PIXMAPS: &str = "/usr/share/pixmaps";

/// The base directories icon themes are looked for in, most important
/// first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPath {
    dirs: Vec<PathBuf>,
}

impl SearchPath {
    /// The specification's base directories for `dirs`: `~/.icons`, then
    /// `icons` in each data directory (`XDG_DATA_HOME` first, then
    /// `XDG_DATA_DIRS`), then `/usr/share/pixmaps`. Each directory appears
    /// once, where it first does.
    #[must_use]
    pub fn new(dirs: &BaseDirs) -> Self {
        let home = dirs.home.iter().map(|home| home.join(".icons"));
        let data = dirs.data_paths("icons");
        Self::from_dirs(home.chain(data).chain([PathBuf::from(PIXMAPS)]).collect())
    }

    /// Exactly `dirs`, most important first, each once.
    #[must_use]
    pub fn from_dirs(dirs: Vec<PathBuf>) -> Self {
        let mut unique: Vec<PathBuf> = Vec::with_capacity(dirs.len());
        for dir in dirs {
            if !unique.contains(&dir) {
                unique.push(dir);
            }
        }
        Self { dirs: unique }
    }

    /// The directories, most important first.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Whether a theme called `name` is installed: whether one of the
    /// directories has its `index.theme`, which is what makes a theme by
    /// the specification. One `stat` per directory; nothing is read.
    #[must_use]
    pub fn has_theme(&self, name: &str) -> bool {
        valid_theme_name(name)
            && self
                .dirs
                .iter()
                .any(|dir| dir.join(name).join("index.theme").is_file())
    }
}

/// Whether `name` can name a theme directory: not empty, not a path.
fn valid_theme_name(name: &str) -> bool {
    !(name.is_empty() || name.contains('/') || name == "." || name == "..")
}

impl Index {
    /// Builds the index for lookups in `theme`: reads the `index.theme` of
    /// `theme`, of every theme it inherits from and of `hicolor`, lists
    /// their icon directories in every base directory, and lists the icons
    /// directly in the base directories.
    ///
    /// Nothing here fails: a theme that is missing or broken is skipped,
    /// and what was wrong is in [`problems`](Index::problems).
    #[must_use]
    pub fn load(search_path: &SearchPath, theme: &str) -> Self {
        Self::build(search_path, theme, Listing::Icons)
    }

    /// The themes a lookup in `theme` consults (see
    /// [`consulted`](Index::consulted)), reading only their `index.theme`
    /// files.
    pub(crate) fn consulted_by(search_path: &SearchPath, theme: &str) -> Vec<String> {
        Self::build(search_path, theme, Listing::ThemesOnly).consulted
    }

    fn build(search_path: &SearchPath, theme: &str, listing: Listing) -> Self {
        let mut index = Self {
            search_path: search_path.dirs.clone(),
            theme: theme.to_owned(),
            consulted: Vec::new(),
            themes: HashMap::new(),
            unthemed: NameTable::default(),
            problems: Vec::new(),
        };
        let mut reader = Reader {
            listing,
            lister: Lister::new(),
            path: PathBuf::new(),
        };
        index.consult(theme, &mut reader);
        index.consult(HICOLOR, &mut reader);
        if listing == Listing::ThemesOnly {
            return index;
        }
        index.report_cycles();
        let mut unthemed = TableBuilder::default();
        for base in 0..index.search_path.len() {
            let dir = &index.search_path[base];
            let listed = reader
                .lister
                .list(dir, |name, format| unthemed.push(name, 0, base, format));
            if let Some((path, kind)) = unreadable(listed, dir) {
                index.problem(theme, Some(path), kind);
            }
        }
        index.report_limits(theme, &unthemed);
        index.unthemed = unthemed.finish();
        index
    }

    /// Loads `theme` and, depth first, its parents, each once.
    fn consult(&mut self, theme: &str, reader: &mut Reader) {
        if self.consulted.iter().any(|seen| seen == theme) {
            return;
        }
        self.consulted.push(theme.to_owned());
        let Some(loaded) = self.load_theme(theme, reader) else {
            return;
        };
        let parents = loaded.theme.inherits.clone();
        self.themes.insert(theme.to_owned(), loaded);
        for parent in parents {
            self.consult(&parent, reader);
        }
    }

    fn load_theme(&mut self, name: &str, reader: &mut Reader) -> Option<Indexed> {
        if !valid_theme_name(name) {
            self.problem(name, None, ProblemKind::InvalidName);
            return None;
        }
        let theme = self.read_theme(name)?;
        if reader.listing == Listing::ThemesOnly {
            let icons = NameTable::default();
            return Some(Indexed { theme, icons });
        }
        let mut icons = TableBuilder::default();
        for (directory, dir) in theme.directories.iter().enumerate() {
            for base in 0..self.search_path.len() {
                let path = &mut reader.path;
                path.as_mut_os_string().clear();
                path.push(&self.search_path[base]);
                path.push(name);
                path.push(&dir.path);
                let listed = reader.lister.list(path, |icon, format| {
                    icons.push(icon, directory, base, format);
                });
                if let Some((path, kind)) = unreadable(listed, &reader.path) {
                    self.problem(name, Some(path), kind);
                }
            }
        }
        self.report_limits(name, &icons);
        let icons = icons.finish();
        Some(Indexed { theme, icons })
    }

    /// The theme from the first base directory with a readable
    /// `index.theme`, as the specification says.
    fn read_theme(&mut self, name: &str) -> Option<Theme> {
        for base in 0..self.search_path.len() {
            let path = self.search_path[base].join(name).join("index.theme");
            let contents = match fs::read(&path) {
                Ok(contents) => contents,
                Err(error) if gone(&error) => continue,
                Err(error) => {
                    self.problem(name, Some(path), ProblemKind::unreadable(&error));
                    continue;
                }
            };
            let mut found = Vec::new();
            let theme = Theme::parse(name, &contents, &mut found);
            for kind in found {
                self.problem(name, Some(path.clone()), kind);
            }
            return theme;
        }
        self.problem(name, None, ProblemKind::Missing);
        None
    }

    /// Reports each limit of the compact form that `table` passed, once.
    fn report_limits(&mut self, theme: &str, table: &TableBuilder) {
        for limit in table.passed() {
            let kind = ProblemKind::TooLarge {
                what: limit.what(),
                limit: limit.limit(),
            };
            self.problem(theme, None, kind);
        }
    }

    /// Reports each cycle in `Inherits` once, from the first theme on it.
    fn report_cycles(&mut self) {
        let mut cycles: Vec<Vec<String>> = Vec::new();
        for start in &self.consulted {
            let mut path = vec![start.clone()];
            self.find_cycle(&mut path, &mut cycles);
        }
        for cycle in cycles {
            self.problem(
                &cycle[0].clone(),
                None,
                ProblemKind::InheritanceCycle { cycle },
            );
        }
    }

    fn find_cycle(&self, path: &mut Vec<String>, cycles: &mut Vec<Vec<String>>) {
        let Some(indexed) = path.last().and_then(|name| self.themes.get(name)) else {
            return;
        };
        for parent in &indexed.theme.inherits {
            if let Some(start) = path.iter().position(|seen| seen == parent) {
                let cycle = &path[start..];
                // The same cycle, found from another of its themes, is
                // a rotation of one already reported.
                let known = cycles.iter().any(|known| same_cycle(known, cycle));
                if start == 0 && !known {
                    cycles.push(cycle.to_vec());
                }
                continue;
            }
            path.push(parent.clone());
            self.find_cycle(path, cycles);
            path.pop();
        }
    }

    fn problem(&mut self, theme: &str, path: Option<PathBuf>, kind: ProblemKind) {
        self.problems.push(Problem {
            theme: theme.to_owned(),
            path,
            kind,
        });
    }
}

/// What a build reads directories with: one listing buffer and one path,
/// reused for every directory.
#[derive(Debug)]
struct Reader {
    listing: Listing,
    lister: Lister,
    path: PathBuf,
}

/// What a listing of `dir` that failed says: nothing for a directory that
/// is not there, which has no icons; else the problem. What a listing
/// lists, links included, is in [`crate::listing`].
fn unreadable(listed: io::Result<()>, dir: &Path) -> Option<(PathBuf, ProblemKind)> {
    match listed {
        Ok(()) => None,
        Err(error) if gone(&error) => None,
        Err(error) => Some((dir.to_owned(), ProblemKind::unreadable(&error))),
    }
}

/// How much of the themes a build reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// Every icon directory, and the base directories' own icons.
    Icons,
    /// Only the `index.theme` files, to learn which themes are consulted.
    ThemesOnly,
}

/// Whether `a` and `b` are the same cycle, starting at different themes.
fn same_cycle(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && (0..a.len()).any(|shift| a.iter().cycle().skip(shift).take(a.len()).eq(b))
}

/// Whether an error means the file or directory is not there: missing, or
/// a path through something that is not a directory.
fn gone(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

#[cfg(test)]
#[path = "load_reference_tests.rs"]
mod reference_tests;
