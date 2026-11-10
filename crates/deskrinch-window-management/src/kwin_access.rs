//! How a client gets KWin's window-management protocol.
//!
//! KWin offers `org_kde_plasma_window_management` only to clients it has
//! granted it to; to every other client the global is simply absent, and
//! [`connect`](crate::connect) fails with
//! [`ConnectError::NotGranted`](crate::ConnectError::NotGranted). This is
//! how KWin 6 decides (read from its source, `src/wayland_server.cpp` and
//! `src/utils/serviceutils.h`, and checked against KWin 6.6.6):
//!
//! 1. It takes the client's process id from the Wayland socket's
//!    credentials and its executable from the `/proc/<pid>/exe` link, with
//!    symbolic links resolved.
//! 2. It looks through the installed applications, the `.desktop` files
//!    under `$XDG_DATA_HOME/applications` and each `$XDG_DATA_DIRS`
//!    `/applications`, through KDE's service cache (built and refreshed by
//!    KWin itself). Only applications shown in the current desktop count: a
//!    file with `OnlyShowIn` or `NotShowIn` that excludes KWin's
//!    `XDG_CURRENT_DESKTOP` is skipped, and so is one with `Hidden=true`.
//! 3. For each, it takes the first word of `Exec`, resolves it as a file
//!    path with symbolic links followed, and compares it with the
//!    executable. A bare command name is resolved against KWin's working
//!    directory, not `PATH`, so it never matches: `Exec` must be an absolute
//!    path. Arguments after the first word are ignored.
//! 4. The first matching file's `X-KDE-Wayland-Interfaces` list is the set
//!    of restricted globals the client may see.
//!
//! KWin looks once per connection, when the client first lists the globals,
//! so a file installed while KWin runs applies to the next connection.
//! Sandboxed clients (a `wp_security_context_v1` app id, as Flatpak sets)
//! are matched by that app id's desktop file instead. Setting
//! `KWIN_WAYLAND_NO_PERMISSION_CHECKS=1` in KWin's own environment lifts the
//! check for every client; it is meant for testing a nested KWin and must
//! never be set in a real session.
//!
//! # An installed deskrinch
//!
//! Each executable that connects installs a desktop file whose `Exec` is that
//! executable's absolute installed path, for example
//! `/usr/share/applications/org.deskrinch.Panel.desktop` with
//! `Exec=/usr/bin/deskrinch-panel`. [`DesktopEntry`] writes it:
//!
//! ```text
//! [Desktop Entry]
//! Type=Application
//! Name=deskrinch panel
//! Exec=/usr/bin/deskrinch-panel
//! NoDisplay=true
//! X-KDE-Wayland-Interfaces=org_kde_plasma_window_management
//! ```
//!
//! `NoDisplay=true` keeps it out of application menus without hiding it from
//! KWin. Leave out `OnlyShowIn`, which would hide it from KWin under any
//! other `XDG_CURRENT_DESKTOP`.
//!
//! # A development build
//!
//! The same rule, with the build's own path: install a desktop file whose
//! `Exec` is the absolute path of the binary Cargo built (for example
//! `target/debug/deskrinch-panel` in the checkout), into the data directory
//! of the session it runs in. Rebuilding keeps the path, so the file keeps
//! working, but a binary replaced while it runs no longer matches until it is
//! restarted (its `/proc` link then names a deleted file). Do it in a nested
//! KWin with its own `XDG_DATA_HOME` rather than in the real session:
//! `cargo xtask compositor-tests` does exactly that for this crate's tests,
//! with [`DesktopEntry`], and with KWin's check left on.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The restricted global this crate needs from KWin.
pub const INTERFACE: &str = "org_kde_plasma_window_management";

/// Characters the desktop entry specification reserves in `Exec`, plus `%`
/// (field codes): a path containing one would have to be quoted, and this
/// writer refuses such paths rather than quote them.
const EXEC_RESERVED: &[char] = &[
    ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(', ')',
    '`', '%',
];

/// Why a [`DesktopEntry`] could not be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DesktopEntryError {
    /// KWin compares `Exec` as a path, so it must be absolute.
    #[error("Exec must be an absolute path, not {}", .0.display())]
    ExecNotAbsolute(PathBuf),
    /// Desktop files are UTF-8.
    #[error("Exec path {} is not UTF-8", .0.display())]
    ExecNotUtf8(PathBuf),
    /// The path holds a character that `Exec` would need quoted.
    #[error("Exec path {} contains {character:?}, which this writer does not quote", path.display())]
    ExecNeedsQuoting {
        /// The path.
        path: PathBuf,
        /// The first such character.
        character: char,
    },
    /// A desktop file id is letters, digits, `_`, `-` and `.`, and not
    /// empty.
    #[error("{0:?} is not a desktop file id")]
    InvalidId(String),
    /// `Name` is one non-empty line without control characters.
    #[error("{0:?} is not a valid Name")]
    InvalidName(String),
}

/// The desktop file that grants an executable KWin's window-management
/// protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    id: String,
    name: String,
    exec: String,
}

impl DesktopEntry {
    /// The entry for the executable at `exec`, installed as `<id>.desktop`
    /// and named `name`.
    ///
    /// # Errors
    ///
    /// When `id` is not a desktop file id, `name` is not one line of text, or
    /// `exec` is not an absolute UTF-8 path that needs no quoting.
    pub fn new(id: &str, name: &str, exec: &Path) -> Result<Self, DesktopEntryError> {
        let valid_id = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        if !valid_id {
            return Err(DesktopEntryError::InvalidId(id.to_owned()));
        }
        if name.trim().is_empty() || name.chars().any(char::is_control) {
            return Err(DesktopEntryError::InvalidName(name.to_owned()));
        }
        if !exec.is_absolute() {
            return Err(DesktopEntryError::ExecNotAbsolute(exec.to_owned()));
        }
        let exec_text = exec
            .to_str()
            .ok_or_else(|| DesktopEntryError::ExecNotUtf8(exec.to_owned()))?;
        if let Some(character) = exec_text
            .chars()
            .find(|c| c.is_control() || EXEC_RESERVED.contains(c))
        {
            return Err(DesktopEntryError::ExecNeedsQuoting {
                path: exec.to_owned(),
                character,
            });
        }
        Ok(Self {
            id: id.to_owned(),
            name: name.to_owned(),
            exec: exec_text.to_owned(),
        })
    }

    /// The file's name: `<id>.desktop`.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.desktop", self.id)
    }

    /// The file's contents.
    #[must_use]
    pub fn contents(&self) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={}\n\
             Exec={}\n\
             NoDisplay=true\n\
             X-KDE-Wayland-Interfaces={INTERFACE}\n",
            self.name, self.exec,
        )
    }

    /// Writes the file into `data_dir`'s `applications` directory (creating
    /// it), where `data_dir` is `$XDG_DATA_HOME` or one of `$XDG_DATA_DIRS`.
    /// Returns the file's path.
    ///
    /// # Errors
    ///
    /// When the directory or the file cannot be written.
    pub fn install(&self, data_dir: &Path) -> io::Result<PathBuf> {
        let applications = data_dir.join("applications");
        fs::create_dir_all(&applications)?;
        let path = applications.join(self.file_name());
        fs::write(&path, self.contents())?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> DesktopEntry {
        DesktopEntry::new(
            "org.deskrinch.Panel",
            "deskrinch panel",
            Path::new("/usr/bin/deskrinch-panel"),
        )
        .unwrap()
    }

    #[test]
    fn writes_the_documented_template() {
        assert_eq!(
            panel().contents(),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=deskrinch panel\n\
             Exec=/usr/bin/deskrinch-panel\n\
             NoDisplay=true\n\
             X-KDE-Wayland-Interfaces=org_kde_plasma_window_management\n"
        );
        assert_eq!(panel().file_name(), "org.deskrinch.Panel.desktop");
    }

    #[test]
    fn the_module_docs_show_the_same_template() {
        let docs = include_str!("kwin_access.rs");
        let shown = panel()
            .contents()
            .lines()
            .fold(String::new(), |shown, line| shown + "//! " + line + "\n");
        assert!(docs.contains(&shown), "the module docs are out of date");
    }

    #[test]
    fn exec_must_be_absolute() {
        let error = DesktopEntry::new("a", "A", Path::new("deskrinch-panel")).unwrap_err();
        assert!(matches!(error, DesktopEntryError::ExecNotAbsolute(_)));
    }

    #[test]
    fn exec_that_would_need_quoting_is_refused() {
        for path in [
            "/opt/my apps/panel",
            "/opt/50%/panel",
            "/opt/a\"b",
            "/opt/a\\b",
        ] {
            let error = DesktopEntry::new("a", "A", Path::new(path)).unwrap_err();
            assert!(
                matches!(error, DesktopEntryError::ExecNeedsQuoting { .. }),
                "{path}: {error:?}"
            );
        }
    }

    #[test]
    fn exec_must_be_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"/opt/\xff"));
        let error = DesktopEntry::new("a", "A", path).unwrap_err();
        assert!(matches!(error, DesktopEntryError::ExecNotUtf8(_)));
    }

    #[test]
    fn id_and_name_are_checked() {
        let exec = Path::new("/usr/bin/x");
        for id in ["", "has space", "slash/id", "ünïcode"] {
            assert_eq!(
                DesktopEntry::new(id, "A", exec),
                Err(DesktopEntryError::InvalidId(id.to_owned()))
            );
        }
        for name in ["", "  ", "two\nlines"] {
            assert_eq!(
                DesktopEntry::new("a", name, exec),
                Err(DesktopEntryError::InvalidName(name.to_owned()))
            );
        }
    }

    #[test]
    fn installs_into_the_applications_directory() {
        let data = tempfile::tempdir().unwrap();
        let path = panel().install(data.path()).unwrap();
        assert_eq!(
            path,
            data.path().join("applications/org.deskrinch.Panel.desktop")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), panel().contents());
    }
}
