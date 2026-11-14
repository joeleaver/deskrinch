//! Reading the `applications` directories from disk into a [`Catalog`].

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use deskrinch_keyfile::Locale;

use crate::catalog::{Catalog, Found, Program};
use crate::entry::{DesktopEntry, DesktopId};
use crate::env::Environment;
use crate::error::{Problem, ProblemKind, ScanError};

impl Catalog {
    /// Scans the `applications` directory of every data directory in `env`.
    ///
    /// A directory that does not exist or cannot be listed contributes
    /// nothing; a file that cannot be read or parsed is skipped and listed
    /// in [`Catalog::problems`]. Symbolic links are followed, and a
    /// directory reached twice is read once. Each entry's translated keys
    /// are resolved for `env`'s locales ([`DesktopEntry::parse`]).
    #[must_use]
    pub fn scan(env: &Environment) -> Self {
        let mut found = Vec::new();
        let mut visited = HashSet::new();
        for dir in application_dirs(env) {
            walk(&dir, &dir, &env.locales, &mut visited, &mut found);
        }
        Self::build(found, env, |program| program_exists(program, &env.path))
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
fn walk(
    root: &Path,
    dir: &Path,
    locales: &[Locale],
    visited: &mut HashSet<PathBuf>,
    found: &mut Vec<Found>,
) {
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
            walk(root, &path, locales, visited, found);
        } else if metadata.is_file() && path.extension().is_some_and(|ext| ext == "desktop") {
            let id = path
                .strip_prefix(root)
                .ok()
                .and_then(DesktopId::from_relative_path);
            if let Some(id) = id {
                found.push(read_entry(id, path, locales));
            }
        }
    }
}

/// Reads and parses one file, for `locales`.
pub(crate) fn read_entry(id: DesktopId, path: PathBuf, locales: &[Locale]) -> Found {
    fs::read(&path)
        .map_err(|error| ScanError::Read(Arc::new(error)))
        .and_then(|bytes| Ok(DesktopEntry::parse(id, path.clone(), bytes, locales)?))
        .map_err(|error| Problem {
            path,
            kind: ProblemKind::Skipped(error),
        })
}

/// Whether `program` is an executable file, with `path` as the `PATH`
/// searched.
///
/// A `TryExec` value is an absolute path or is looked up in `PATH`, as the
/// specification says. An `Exec` program is found the way the launch's
/// `execvp` finds it: an absolute path as it is, a path containing `/`
/// relative to the working directory the launch runs it in (the entry's
/// `Path`, or else the current directory), and a bare name in `PATH`.
pub(crate) fn program_exists(program: Program<'_>, path: &[PathBuf]) -> bool {
    let in_path = |name: &Path| path.iter().any(|dir| is_executable(&dir.join(name)));
    match program {
        Program::TryExec(program) => {
            let program = Path::new(program);
            if program.is_absolute() {
                is_executable(program)
            } else {
                in_path(program)
            }
        }
        Program::Exec {
            program,
            working_dir,
        } => {
            let as_path = Path::new(program);
            if as_path.is_absolute() {
                is_executable(as_path)
            } else if program.contains('/') {
                match working_dir {
                    Some(dir) => is_executable(&dir.join(as_path)),
                    None => is_executable(as_path),
                }
            } else {
                in_path(as_path)
            }
        }
    }
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
