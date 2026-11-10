//! Which files make up the configuration, and reading them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use deskrinch_xdg::BaseDirs;

use crate::section::{Resolved, Section, Settings};
use crate::snapshot::Snapshot;

/// The configuration file's path under each configuration directory.
pub const FILE: &str = "deskrinch/config.toml";

/// The files the configuration is layered from: the system files, lowest
/// first, then the user's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    system: Vec<PathBuf>,
    user: Option<PathBuf>,
}

impl Sources {
    /// [`FILE`] in each of `dirs.config_dirs` (the last, least important,
    /// lowest) and then in `dirs.config_home`.
    #[must_use]
    pub fn new(dirs: &BaseDirs) -> Self {
        Self {
            system: dirs
                .config_dirs
                .iter()
                .rev()
                .map(|dir| dir.join(FILE))
                .collect(),
            user: dirs.config_home.as_ref().map(|dir| dir.join(FILE)),
        }
    }

    /// Exactly these files: `system`, lowest first, then `user`.
    #[must_use]
    pub fn from_files(system: Vec<PathBuf>, user: Option<PathBuf>) -> Self {
        Self { system, user }
    }

    /// The user's file: the top layer, and the one a settings editor
    /// writes.
    #[must_use]
    pub fn user_file(&self) -> Option<&Path> {
        self.user.as_deref()
    }

    /// Every file, lowest layer first.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        self.system.iter().chain(&self.user).map(PathBuf::as_path)
    }
}

impl Snapshot {
    /// Reads every file of `sources` now. A missing file is an empty
    /// layer; one that cannot be read is reported and skipped.
    #[must_use]
    pub fn read(sources: &Sources) -> Self {
        Self::from_reads(
            sources
                .files()
                .map(|path| (path.to_path_buf(), read_file(path))),
        )
    }
}

fn read_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Reads the files of `sources` and resolves `section` from them: the
/// one-shot form of [`Section::resolve`]. Problems in the files are never
/// errors: they are [`Resolved::diagnostics`].
#[must_use]
pub fn load<T: Settings>(sources: &Sources, mut section: Section<T>) -> Resolved<T> {
    section.resolve(&Snapshot::read(sources))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn layers_system_files_below_the_users() {
        let dirs = BaseDirs::from_vars(|name| match name {
            "HOME" => Some(OsString::from("/home/u")),
            "XDG_CONFIG_DIRS" => Some(OsString::from("/etc/xdg/vendor:/etc/xdg")),
            _ => None,
        });
        let sources = Sources::new(&dirs);
        let files: Vec<_> = sources.files().collect();
        assert_eq!(
            files,
            [
                Path::new("/etc/xdg/deskrinch/config.toml"),
                Path::new("/etc/xdg/vendor/deskrinch/config.toml"),
                Path::new("/home/u/.config/deskrinch/config.toml"),
            ]
        );
        assert_eq!(sources.user_file(), Some(files[2]));
    }
}
