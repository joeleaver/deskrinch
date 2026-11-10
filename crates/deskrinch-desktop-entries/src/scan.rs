//! Reading the `applications` directories from disk into a [`Catalog`].

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::catalog::{Catalog, Found};
use crate::entry::{DesktopEntry, DesktopId};
use crate::env::Environment;
use crate::error::{ParseError, Problem, ScanError};

impl Catalog {
    /// Scans the `applications` directory of every data directory in `env`.
    ///
    /// A directory that does not exist or cannot be listed contributes
    /// nothing; a file that cannot be read or parsed is skipped and listed
    /// in [`Catalog::problems`]. Symbolic links are followed, and a
    /// directory reached twice is read once.
    #[must_use]
    pub fn scan(env: &Environment) -> Self {
        let mut found = Vec::new();
        let mut visited = HashSet::new();
        for dir in application_dirs(env) {
            walk(&dir, &dir, &mut visited, &mut found);
        }
        Self::build(found, env, |program| try_exec_exists(program, &env.path))
    }
}

/// The `applications` directory of every data directory, in precedence
/// order.
pub(crate) fn application_dirs(env: &Environment) -> Vec<PathBuf> {
    env.data_dirs_in_order()
        .map(|dir| dir.join("applications"))
        .collect()
}

/// Appends every `.desktop` file below `dir`, in sorted order, to `found`.
fn walk(root: &Path, dir: &Path, visited: &mut HashSet<PathBuf>, found: &mut Vec<Found>) {
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = listing.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        // `metadata` follows symbolic links.
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            walk(root, &path, visited, found);
        } else if metadata.is_file() && path.extension().is_some_and(|ext| ext == "desktop") {
            let id = path
                .strip_prefix(root)
                .ok()
                .and_then(DesktopId::from_relative_path);
            if let Some(id) = id {
                found.push(read_entry(id, path));
            }
        }
    }
}

/// Reads and parses one file.
pub(crate) fn read_entry(id: DesktopId, path: PathBuf) -> Found {
    let text = match fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| ScanError::Parse(ParseError::NotUtf8)),
        Err(error) => Err(ScanError::Read(Arc::new(error))),
    };
    text.and_then(|text| Ok(DesktopEntry::parse(id, path.clone(), &text)?))
        .map_err(|error| Problem { path, error })
}

/// Whether `program` (an absolute path, or a name looked up in `path`) is
/// an executable file.
pub(crate) fn try_exec_exists(program: &str, path: &[PathBuf]) -> bool {
    let program = Path::new(program);
    if program.is_absolute() {
        return is_executable(program);
    }
    path.iter().any(|dir| is_executable(&dir.join(program)))
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
