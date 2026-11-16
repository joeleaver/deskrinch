//! `--state DIR` and `--config DIR`: each run's state directory
//! (`XDG_STATE_HOME`) or configuration directory (`XDG_CONFIG_HOME`)
//! starts with a copy of `DIR`'s contents, so that what is measured is a
//! program with that state (the launcher's record of starts,
//! `deskrinch/starts.toml`) or that configuration (the wallpaper's
//! `[wallpaper]` with a picture, in `deskrinch/config.toml`).
//!
//! The copy is made into the session's directory once the compositor has
//! started and before the program does; nothing is written back to `DIR`.
//! A `DIR` that is not a readable directory fails before any session. The
//! report's header names each seeded directory and the files it held.

use std::fs;
use std::io;
use std::path::Path;

use crate::error::Error;

/// Which of the session's directories is seeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layer {
    /// `--state`: `XDG_STATE_HOME`.
    State,
    /// `--config`: `XDG_CONFIG_HOME`.
    Configuration,
}

impl Layer {
    /// What it is called in the report and in errors.
    fn name(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Configuration => "configuration",
        }
    }

    /// The option that seeds it.
    fn option(self) -> &'static str {
        match self {
            Self::State => "--state",
            Self::Configuration => "--config",
        }
    }

    /// The session's variable for it.
    fn variable(self) -> &'static str {
        match self {
            Self::State => "XDG_STATE_HOME",
            Self::Configuration => "XDG_CONFIG_HOME",
        }
    }
}

/// Checks, before anything starts, that `dir` is a directory that can be
/// read.
pub(super) fn check(layer: Layer, dir: &Path) -> Result<(), Error> {
    let unreadable = |source| Error::Io {
        what: format!("read the {} to seed, {}", layer.name(), dir.display()),
        source,
    };
    if !fs::metadata(dir).map_err(unreadable)?.is_dir() {
        return Err(unreadable(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} names a directory", layer.option()),
        )));
    }
    fs::read_dir(dir).map_err(unreadable)?;
    Ok(())
}

/// Copies `dir`'s contents into the session's directory for `layer` under
/// `root`, keeping each file's permissions.
pub(super) fn seed(layer: Layer, dir: &Path, root: &Path) -> Result<(), Error> {
    let target = crate::session::session_dir(root, layer.variable())?;
    copy_tree(dir, &target).map_err(|source| Error::Io {
        what: format!(
            "seed the session's {} {} from {}",
            layer.name(),
            target.display(),
            dir.display()
        ),
        source,
    })
}

/// The report's line naming the seeded `dir` for `layer`, and the files it
/// held (relative to it, sorted).
pub(super) fn note(layer: Layer, dir: &Path) -> String {
    let mut files = Vec::new();
    list(dir, Path::new(""), &mut files);
    files.sort();
    let held = if files.is_empty() {
        "no files".to_owned()
    } else {
        files.join(", ")
    };
    format!(
        "{} ({}) seeded from {}: {held}",
        layer.name(),
        layer.variable(),
        dir.display()
    )
}

/// The files under `dir`, each as `prefix` joined with its path in `dir`.
fn list(dir: &Path, prefix: &Path, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let relative = prefix.join(entry.file_name());
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            list(&entry.path(), &relative, files);
        } else {
            files.push(relative.display().to_string());
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_names_the_layer_the_directory_and_its_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("deskrinch")).unwrap();
        fs::write(dir.path().join("deskrinch/config.toml"), "").unwrap();
        fs::write(dir.path().join("kdeglobals"), "").unwrap();
        assert_eq!(
            note(Layer::Configuration, dir.path()),
            format!(
                "configuration (XDG_CONFIG_HOME) seeded from {}: deskrinch/config.toml, kdeglobals",
                dir.path().display()
            )
        );
        let empty = tempfile::tempdir().unwrap();
        assert!(note(Layer::State, empty.path()).ends_with(": no files"));
    }

    #[test]
    fn a_seed_is_copied_into_its_own_directory_of_the_session() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("deskrinch")).unwrap();
        fs::write(dir.path().join("deskrinch/config.toml"), "x = 1\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        seed(Layer::Configuration, dir.path(), root.path()).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("config/deskrinch/config.toml")).unwrap(),
            "x = 1\n"
        );
        assert!(!root.path().join("state").exists());
    }

    #[test]
    fn a_seed_that_is_not_a_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        for layer in [Layer::State, Layer::Configuration] {
            let error = check(layer, &file).unwrap_err().to_string();
            assert!(
                error.contains(&format!("the {} to seed", layer.name())),
                "{error}"
            );
            assert!(check(layer, &dir.path().join("missing")).is_err());
        }
    }
}
