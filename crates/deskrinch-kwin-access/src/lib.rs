//! How a client gets what KWin restricts: some Wayland globals and some
//! D-Bus interfaces.
//!
//! KWin offers some Wayland globals (`org_kde_plasma_window_management`,
//! for one) only to clients it has granted them to; to every other client
//! the global is simply absent. It answers some D-Bus interfaces
//! (`org.kde.KWin.ScreenShot2`, for one) only for callers it has granted them
//! to; every other caller gets the error
//! `org.kde.KWin.ScreenShot2.Error.NoAuthorized`. Both checks decide the same
//! way, from the caller's installed desktop file (read from KWin 6's source,
//! `src/wayland_server.cpp`, `src/utils/serviceutils.h` and the screenshot
//! plugin, and checked against KWin 6.6.6):
//!
//! 1. It takes the client's process id (from the Wayland socket's
//!    credentials, or from the bus for a D-Bus caller) and its executable
//!    from the `/proc/<pid>/exe` link, with symbolic links resolved.
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
//! 4. The **first** matching file decides. Its `X-KDE-Wayland-Interfaces`
//!    list is the set of restricted globals the client may see, and its
//!    `X-KDE-DBUS-Restricted-Interfaces` list the set of restricted D-Bus
//!    interfaces it may call.
//!
//! Because the first match decides, a second file for the same executable
//! adds nothing: **one executable gets one file, carrying everything it
//! needs**. A [`DesktopEntry`] is built from the constants of the crates the
//! executable uses (each crate that needs a restricted interface exports its
//! name), never as one file per crate.
//!
//! KWin looks for the Wayland globals once per connection, when the client
//! first lists them, and for a D-Bus interface on each call; so a file
//! installed while KWin runs applies to the next connection or call, once
//! KWin's service cache has seen it. Sandboxed clients (a
//! `wp_security_context_v1` app id, as Flatpak sets) are matched by that app
//! id's desktop file instead.
//!
//! KWin can also lift each check for every client:
//! `KWIN_WAYLAND_NO_PERMISSION_CHECKS=1` and
//! `KWIN_SCREENSHOT_NO_PERMISSION_CHECKS=1` in KWin's own environment. They
//! are meant for testing a nested KWin and must never be set in a real
//! session; deskrinch's tests leave both unset and are granted what they use
//! the way an installed deskrinch is.
//!
//! # An installed deskrinch
//!
//! Each executable that needs a restricted interface installs a desktop file
//! whose `Exec` is that executable's absolute installed path, for example
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
//! `cargo xtask compositor-tests` does exactly that for each test binary it
//! runs, with one [`DesktopEntry`] per binary, and with KWin's checks left
//! on.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
    /// A Wayland global or D-Bus interface name is letters, digits, `_` and
    /// `.`, and not empty: a list entry needs no escaping.
    #[error("{0:?} is not an interface name")]
    InvalidInterface(String),
    /// The entry grants nothing, so installing it would only hide the
    /// executable from a later file that does.
    #[error("the entry grants no interface")]
    GrantsNothing,
}

/// The desktop file that grants an executable what KWin restricts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    id: String,
    name: String,
    exec: String,
    wayland: Vec<String>,
    dbus: Vec<String>,
}

impl DesktopEntry {
    /// The entry for the executable at `exec`, installed as `<id>.desktop`
    /// and named `name`, granting the Wayland globals `wayland` and the D-Bus
    /// interfaces `dbus`. Each list keeps its order, without repeats.
    ///
    /// # Errors
    ///
    /// When `id` is not a desktop file id, `name` is not one line of text,
    /// `exec` is not an absolute UTF-8 path that needs no quoting, an
    /// interface is not a plain name, or both lists are empty.
    pub fn new(
        id: &str,
        name: &str,
        exec: &Path,
        wayland: &[&str],
        dbus: &[&str],
    ) -> Result<Self, DesktopEntryError> {
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
        if wayland.is_empty() && dbus.is_empty() {
            return Err(DesktopEntryError::GrantsNothing);
        }
        Ok(Self {
            id: id.to_owned(),
            name: name.to_owned(),
            exec: exec_text.to_owned(),
            wayland: interface_list(wayland)?,
            dbus: interface_list(dbus)?,
        })
    }

    /// The file's name: `<id>.desktop`.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.desktop", self.id)
    }

    /// The file's contents. A key whose list is empty is left out.
    #[must_use]
    pub fn contents(&self) -> String {
        let mut contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name={}\n\
             Exec={}\n\
             NoDisplay=true\n",
            self.name, self.exec,
        );
        for (key, list) in [
            ("X-KDE-Wayland-Interfaces", &self.wayland),
            ("X-KDE-DBUS-Restricted-Interfaces", &self.dbus),
        ] {
            if !list.is_empty() {
                contents.push_str(key);
                contents.push('=');
                contents.push_str(&list.join(","));
                contents.push('\n');
            }
        }
        contents
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

/// `names`, checked, in order and without repeats.
fn interface_list(names: &[&str]) -> Result<Vec<String>, DesktopEntryError> {
    let mut list: Vec<String> = Vec::with_capacity(names.len());
    for &name in names {
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'));
        if !valid {
            return Err(DesktopEntryError::InvalidInterface(name.to_owned()));
        }
        if !list.iter().any(|known| known == name) {
            list.push(name.to_owned());
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW_MANAGEMENT: &str = "org_kde_plasma_window_management";
    const SCREENSHOT: &str = "org.kde.KWin.ScreenShot2";

    fn panel() -> DesktopEntry {
        DesktopEntry::new(
            "org.deskrinch.Panel",
            "deskrinch panel",
            Path::new("/usr/bin/deskrinch-panel"),
            &[WINDOW_MANAGEMENT],
            &[],
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
    fn the_crate_docs_show_the_same_template() {
        let docs = include_str!("lib.rs");
        let shown = panel()
            .contents()
            .lines()
            .fold(String::new(), |shown, line| shown + "//! " + line + "\n");
        assert!(docs.contains(&shown), "the crate docs are out of date");
    }

    #[test]
    fn one_file_carries_both_lists() {
        let entry = DesktopEntry::new(
            "t",
            "T",
            Path::new("/bin/t"),
            &[
                WINDOW_MANAGEMENT,
                "zkde_screencast_unstable_v1",
                WINDOW_MANAGEMENT,
            ],
            &[SCREENSHOT],
        )
        .unwrap();
        assert_eq!(
            entry.contents(),
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=T\n\
             Exec=/bin/t\n\
             NoDisplay=true\n\
             X-KDE-Wayland-Interfaces=org_kde_plasma_window_management,zkde_screencast_unstable_v1\n\
             X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n"
        );
        let dbus_only = DesktopEntry::new("t", "T", Path::new("/bin/t"), &[], &[SCREENSHOT]);
        assert!(
            !dbus_only
                .unwrap()
                .contents()
                .contains("X-KDE-Wayland-Interfaces")
        );
    }

    #[test]
    fn an_entry_must_grant_something() {
        assert_eq!(
            DesktopEntry::new("t", "T", Path::new("/bin/t"), &[], &[]),
            Err(DesktopEntryError::GrantsNothing)
        );
    }

    #[test]
    fn interfaces_are_plain_names() {
        for name in ["", "a,b", "a b", "a;b", "a\n"] {
            assert_eq!(
                DesktopEntry::new("t", "T", Path::new("/bin/t"), &[name], &[]),
                Err(DesktopEntryError::InvalidInterface(name.to_owned()))
            );
            assert_eq!(
                DesktopEntry::new("t", "T", Path::new("/bin/t"), &[], &[name]),
                Err(DesktopEntryError::InvalidInterface(name.to_owned()))
            );
        }
    }

    #[test]
    fn exec_must_be_absolute() {
        let error = DesktopEntry::new("a", "A", Path::new("deskrinch-panel"), &[SCREENSHOT], &[])
            .unwrap_err();
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
            let error =
                DesktopEntry::new("a", "A", Path::new(path), &[], &[SCREENSHOT]).unwrap_err();
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
        let error = DesktopEntry::new("a", "A", path, &[], &[SCREENSHOT]).unwrap_err();
        assert!(matches!(error, DesktopEntryError::ExecNotUtf8(_)));
    }

    #[test]
    fn id_and_name_are_checked() {
        let exec = Path::new("/usr/bin/x");
        for id in ["", "has space", "slash/id", "ünïcode"] {
            assert_eq!(
                DesktopEntry::new(id, "A", exec, &[], &[SCREENSHOT]),
                Err(DesktopEntryError::InvalidId(id.to_owned()))
            );
        }
        for name in ["", "  ", "two\nlines"] {
            assert_eq!(
                DesktopEntry::new("a", name, exec, &[], &[SCREENSHOT]),
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
