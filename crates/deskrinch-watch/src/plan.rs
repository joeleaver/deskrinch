//! Which directories to watch for a set of targets, and which changes
//! concern them.
//!
//! Every path here is resolved through symbolic links up to, but not
//! including, its last component, so two spellings of one directory are
//! one watch (inotify gives a directory one watch however it is named).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::Target;

/// The watches a set of targets needs now.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// Each directory watched, and whether recursively.
    pub(crate) watches: BTreeMap<PathBuf, bool>,
    /// The paths whose changes matter: each target where it is written,
    /// and where it points when it is a symbolic link; and whether what is
    /// below it matters to any depth or only its entries.
    interest: Vec<(PathBuf, Depth)>,
}

/// How far below an interesting path a change matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// Anywhere below (a tree; nothing is below a file).
    Any,
    /// Only the directory's own entries.
    Entries,
}

impl Plan {
    pub(crate) fn new(targets: &[Target]) -> Self {
        let mut plan = Self::default();
        for target in targets {
            let path = target.path();
            let located = located(path);
            let resolved = path.canonicalize().ok().filter(|real| *real != located);
            let depth = match target {
                Target::File(_) | Target::Tree(_) => Depth::Any,
                Target::Directory(_) => Depth::Entries,
            };
            for point in std::iter::once(located).chain(resolved) {
                // The directory holding it sees it created, removed,
                // replaced by a rename or re-pointed; its nearest existing
                // ancestor sees the way to it created.
                if let Some(dir) = point.ancestors().skip(1).find(|dir| dir.is_dir()) {
                    plan.add(dir.to_path_buf(), false);
                }
                plan.interest.push((point, depth));
            }
            let recursive = match target {
                Target::File(_) => continue,
                Target::Tree(_) => true,
                Target::Directory(_) => false,
            };
            if let Ok(dir) = path.canonicalize()
                && dir.is_dir()
            {
                plan.add(dir, recursive);
            }
        }
        plan.prune();
        plan
    }

    fn add(&mut self, dir: PathBuf, recursive: bool) {
        *self.watches.entry(dir).or_default() |= recursive;
    }

    /// Drops the watches a recursive one already covers.
    fn prune(&mut self) {
        let recursive: Vec<PathBuf> = self
            .watches
            .iter()
            .filter(|(_, recursive)| **recursive)
            .map(|(dir, _)| dir.clone())
            .collect();
        self.watches.retain(|dir, _| {
            !recursive
                .iter()
                .any(|tree| dir != tree && dir.starts_with(tree))
        });
    }

    /// Whether a change at `path` may concern a target: it is the target,
    /// inside it (for a directory, one of its own entries), or a directory
    /// on the way to it.
    pub(crate) fn concerns(&self, path: &Path) -> bool {
        self.interest.iter().any(|(point, depth)| {
            point.starts_with(path)
                || match depth {
                    Depth::Any => path.starts_with(point),
                    Depth::Entries => path.parent() == Some(point.as_path()),
                }
        })
    }
}

/// `path` with every component but the last resolved through symbolic
/// links, as far as the directories exist.
fn located(path: &Path) -> PathBuf {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return path.to_path_buf();
    };
    for ancestor in parent.ancestors() {
        if let Ok(real) = ancestor.canonicalize() {
            let rest = parent.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return real.join(rest).join(name);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    fn watches(plan: &Plan) -> Vec<(PathBuf, bool)> {
        plan.watches.iter().map(|(d, r)| (d.clone(), *r)).collect()
    }

    #[test]
    fn watches_a_files_directory_or_its_nearest_existing_ancestor() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        fs::create_dir(root.join("present")).unwrap();
        let plan = Plan::new(&[
            Target::File(root.join("present/config.toml")),
            Target::File(root.join("absent/deskrinch/config.toml")),
        ]);
        assert_eq!(
            watches(&plan),
            [(root.clone(), false), (root.join("present"), false)]
        );
        assert!(plan.concerns(&root.join("present/config.toml")));
        assert!(plan.concerns(&root.join("absent")));
        assert!(!plan.concerns(&root.join("present/other.toml")));
        assert!(!plan.concerns(&root.join("present/.config.toml.swp")));
    }

    #[test]
    fn watches_a_tree_recursively_once_it_exists() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let tree = root.join("share/applications");
        let missing = Plan::new(&[Target::Tree(tree.clone())]);
        assert_eq!(watches(&missing), [(root.clone(), false)]);
        fs::create_dir_all(&tree).unwrap();
        let present = Plan::new(&[Target::Tree(tree.clone())]);
        assert_eq!(
            watches(&present),
            [(root.join("share"), false), (tree.clone(), true)]
        );
        assert!(present.concerns(&tree.join("kde/org.kde.foo.desktop")));
        assert!(!present.concerns(&root.join("share/icons")));
    }

    #[test]
    fn follows_symbolic_links_to_their_real_place() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("dotfiles")).unwrap();
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("dotfiles/config.toml"), "").unwrap();
        symlink(root.join("real"), root.join("linked")).unwrap();
        symlink(
            root.join("dotfiles/config.toml"),
            root.join("real/config.toml"),
        )
        .unwrap();
        // Through a linked directory, to a linked file.
        let plan = Plan::new(&[Target::File(root.join("linked/config.toml"))]);
        assert_eq!(
            watches(&plan),
            [(root.join("dotfiles"), false), (root.join("real"), false)]
        );
        assert!(plan.concerns(&root.join("real/config.toml")));
        assert!(plan.concerns(&root.join("dotfiles/config.toml")));
    }

    #[test]
    fn merges_and_prunes_watches_a_recursive_one_covers() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("tree/sub")).unwrap();
        let plan = Plan::new(&[
            Target::File(root.join("tree/sub/file")),
            Target::File(root.join("tree/file")),
            Target::Tree(root.join("tree")),
        ]);
        assert_eq!(
            watches(&plan),
            [(root.clone(), false), (root.join("tree"), true)]
        );
    }

    #[test]
    fn watches_a_directory_for_its_own_entries_only() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let dir = root.join("pixmaps");
        fs::create_dir_all(dir.join("sub")).unwrap();
        let plan = Plan::new(&[Target::Directory(dir.clone())]);
        assert_eq!(
            watches(&plan),
            [(root.clone(), false), (dir.clone(), false)]
        );
        assert!(plan.concerns(&dir.join("foot.png")));
        assert!(plan.concerns(&dir));
        assert!(plan.concerns(&root));
        assert!(!plan.concerns(&dir.join("sub/deep.png")));
        assert!(!plan.concerns(&root.join("other")));
    }
}
