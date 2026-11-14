//! The in-memory description of the themes a lookup consults: each theme's
//! directories, and which icons each directory holds in each base
//! directory. Building it reads files ([`crate::load`]); using it does not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::problem::Problem;
use crate::table::{Location, NameTable};
use crate::theme::Theme;

/// An icon file's format: one of the three the specification allows in
/// themes, in the order a lookup prefers them within one directory, or,
/// for an absolute path a caller gave, any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Format {
    /// `.png`.
    Png,
    /// `.svg`.
    Svg,
    /// `.xpm`.
    Xpm,
    /// Another format, or none the extension names: only ever for an
    /// absolute path given to [`Index::find`]. Whether it can be drawn is
    /// the renderer's business; its extension is in the path.
    Other,
}

impl Format {
    /// The specification's formats, in the order a lookup prefers them.
    pub const ALL: [Self; 3] = [Self::Png, Self::Svg, Self::Xpm];

    /// The file extension, without the dot; `None` for
    /// [`Other`](Self::Other).
    #[must_use]
    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Png => Some("png"),
            Self::Svg => Some("svg"),
            Self::Xpm => Some("xpm"),
            Self::Other => None,
        }
    }

    /// The format a file name's extension names, if it is one of the
    /// specification's, and the name without it: `firefox.png` is
    /// `("firefox", Png)`. Extensions are matched exactly, as GTK matches
    /// them.
    #[must_use]
    pub fn split(file_name: &str) -> Option<(&str, Self)> {
        let (stem, extension) = file_name.rsplit_once('.')?;
        let format = Self::ALL
            .into_iter()
            .find(|format| format.extension() == Some(extension))?;
        (!stem.is_empty()).then_some((stem, format))
    }
}

/// Everything a lookup for one theme needs: that theme, the themes it
/// inherits from, `hicolor`, and the icons outside any theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub(crate) search_path: Vec<PathBuf>,
    pub(crate) theme: String,
    /// Every theme name the lookup consults, in the order it first meets
    /// them, installed or not.
    pub(crate) consulted: Vec<String>,
    pub(crate) themes: HashMap<String, Indexed>,
    /// Icons directly in a base directory, each at directory 0.
    pub(crate) unthemed: NameTable,
    pub(crate) problems: Vec<Problem>,
}

/// One installed theme, and where its icons are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Indexed {
    pub(crate) theme: Theme,
    /// Each icon name's files, in lookup order: by directory (in the
    /// theme's order), then base directory, then format.
    pub(crate) icons: NameTable,
}

impl Index {
    /// The theme lookups start from.
    #[must_use]
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// The base directories, most important first.
    #[must_use]
    pub fn search_path(&self) -> &[PathBuf] {
        &self.search_path
    }

    /// Every theme a lookup may consult, in the order it first meets them:
    /// the theme, its parents depth first, then `hicolor`. Themes that are
    /// not installed are included, so a watcher can wait for them.
    #[must_use]
    pub fn consulted(&self) -> &[String] {
        &self.consulted
    }

    /// The installed theme called `name`, if the index has it.
    #[must_use]
    pub fn installed(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name).map(|indexed| &indexed.theme)
    }

    /// How many icon files the index knows, in the themes and directly in
    /// the base directories.
    #[must_use]
    pub fn files(&self) -> usize {
        let themed = self.themes.values().map(|theme| &theme.icons);
        themed.chain([&self.unthemed]).map(NameTable::files).sum()
    }

    /// What was wrong with the themes, in the order it was found.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// The path of the icon at `location` in `theme`.
    pub(crate) fn themed_path(&self, theme: &Indexed, name: &str, at: Location) -> PathBuf {
        let directory = &theme.theme.directories[at.directory()].path;
        self.base(at)
            .join(&theme.theme.name)
            .join(directory)
            .join(file_name(name, at.format()))
    }

    /// The path of the unthemed icon at `location`.
    pub(crate) fn unthemed_path(&self, name: &str, at: Location) -> PathBuf {
        self.base(at).join(file_name(name, at.format()))
    }

    fn base(&self, at: Location) -> &Path {
        &self.search_path[at.base()]
    }
}

/// An indexed icon's file name. Indexed icons are always in one of the
/// specification's formats, which have an extension.
fn file_name(name: &str, format: Format) -> String {
    match format.extension() {
        Some(extension) => format!("{name}.{extension}"),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_file_names_by_format() {
        assert_eq!(Format::split("firefox.png"), Some(("firefox", Format::Png)));
        assert_eq!(Format::split("a.b.svg"), Some(("a.b", Format::Svg)));
        assert_eq!(Format::split("x.xpm"), Some(("x", Format::Xpm)));
        for none in ["firefox", "firefox.PNG", "f.svgz", ".png", "f.icon"] {
            assert_eq!(Format::split(none), None, "{none}");
        }
    }
}
