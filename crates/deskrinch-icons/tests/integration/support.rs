//! Fixture themes in a temporary directory: base directories `b0`, `b1`
//! and `pixmaps`, most important first.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use deskrinch_icons::{Index, Request, SearchPath};
use tempfile::TempDir;

/// The base directories under one temporary root.
pub(crate) struct Fixture {
    _dir: TempDir,
    pub(crate) root: PathBuf,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for base in ["b0", "b1", "pixmaps"] {
            fs::create_dir(root.join(base)).unwrap();
        }
        Self { _dir: dir, root }
    }

    pub(crate) fn search_path(&self) -> SearchPath {
        SearchPath::from_dirs(vec![self.base("b0"), self.base("b1"), self.base("pixmaps")])
    }

    pub(crate) fn base(&self, base: &str) -> PathBuf {
        self.root.join(base)
    }

    /// Writes `theme`'s `index.theme` into `base`.
    pub(crate) fn theme(&self, base: &str, theme: &str, text: &str) {
        write(&self.base(base).join(theme).join("index.theme"), text);
    }

    /// Writes an icon file, `theme/dir/file` under `base` (or `file`
    /// directly under it when `theme` is empty), and returns its path.
    pub(crate) fn icon(&self, base: &str, theme: &str, dir: &str, file: &str) -> PathBuf {
        let path = self.base(base).join(theme).join(dir).join(file);
        write(&path, "icon");
        path
    }

    pub(crate) fn load(&self, theme: &str) -> Index {
        Index::load(&self.search_path(), theme)
    }
}

/// Writes `text` to `path` in place, creating its directories.
pub(crate) fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A theme whose directories are `name:size:type`, inheriting `inherits`.
pub(crate) fn theme_text(inherits: &str, directories: &[&str]) -> String {
    let names: Vec<&str> = directories
        .iter()
        .map(|d| d.split(':').next().unwrap())
        .collect();
    let mut text = format!(
        "[Icon Theme]\nName=T\nComment=A test theme\nInherits={inherits}\nDirectories={}\n",
        names.join(",")
    );
    for d in directories {
        let parts: Vec<&str> = d.split(':').collect();
        write!(
            text,
            "[{}]\nSize={}\nType={}\n",
            parts[0], parts[1], parts[2]
        )
        .unwrap();
    }
    text
}

/// The path `index` finds for `name` at 48.
pub(crate) fn found(index: &Index, name: &str) -> Option<PathBuf> {
    index
        .lookup(&Request::new(name, 48))
        .ok()
        .map(|icon| icon.path)
}
