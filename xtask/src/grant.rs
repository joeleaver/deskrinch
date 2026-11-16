//! What KWin grants: the shell's components what `deskrinch-shell`'s list
//! says ([`Component::wayland_grants`]), and other programs xtask starts
//! (test binaries, its own input client) what they use ([`client`]). KWin offers
//! a restricted interface only to an executable whose installed desktop
//! file grants it (`deskrinch-kwin-access`). Every xtask that starts such a
//! program under KWin (`desktop`, `measure`, `compositor-tests`, `session
//! --grant`) installs its grant through here; sway and labwc restrict
//! nothing.
//!
//! A grant is installed into the session's data directory before the
//! compositor starts. KWin looks a client's grant up when the client
//! connects, through KDE's service cache, but a file installed while KWin
//! runs is seen only once that cache has been refreshed, at a time KWin
//! chooses (`deskrinch-kwin-access`'s crate doc): installed first, it is
//! there for the first connection.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_shell::{COMPONENTS, Component};

use crate::error::Error;

/// One of the shell's components and its executable, to be granted what
/// the shell grants that component: `cargo xtask session --grant
/// PACKAGE=PATH`, and measure's measured component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComponentGrant {
    /// The component.
    pub(crate) component: &'static Component,
    /// Its executable, links resolved: what KWin sees in `/proc/<pid>/exe`.
    pub(crate) executable: PathBuf,
}

impl ComponentGrant {
    /// The grant `text` (`PACKAGE=PATH`) asks for: the shell's component
    /// whose package is PACKAGE, and PATH's executable.
    ///
    /// # Errors
    ///
    /// When `text` is not `PACKAGE=PATH`, PACKAGE is not one of the shell's
    /// components, or PATH is not a file.
    pub(crate) fn parse(text: &str) -> Result<Self, Error> {
        let (package, path) = text
            .split_once('=')
            .filter(|(package, path)| !package.is_empty() && !path.is_empty())
            .ok_or_else(|| Error::GrantNotUnderstood(text.to_owned()))?;
        let component = component(Path::new(path), Some(package))?
            .ok_or_else(|| Error::GrantNotUnderstood(text.to_owned()))?;
        Ok(Self {
            component,
            executable: executable(OsStr::new(path))?,
        })
    }

    /// Installs it into the data directory `data_home`; whether there was
    /// anything to grant.
    pub(crate) fn install(&self, data_home: &Path) -> Result<bool, Error> {
        install(self.component, &self.executable, data_home)
    }
}

/// `program`'s executable, with links resolved, as KWin compares it: a
/// path as given, or a bare name found on `PATH`.
///
/// # Errors
///
/// When it cannot be found or resolved.
pub(crate) fn executable(program: &OsStr) -> Result<PathBuf, Error> {
    let given = Path::new(program);
    let found = if given.components().count() > 1 {
        Some(given.to_owned())
    } else {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(given))
                .find(|candidate| candidate.is_file())
        })
    };
    let found = found.ok_or_else(|| Error::Io {
        what: format!("find {} to grant it", given.display()),
        source: std::io::ErrorKind::NotFound.into(),
    })?;
    found.canonicalize().map_err(|source| Error::Io {
        what: format!("resolve {}", found.display()),
        source,
    })
}

/// The shell's component named `named` (its package), or else the one
/// whose package is `program`'s file name; `None` when `program` is none of
/// them and none is named.
///
/// # Errors
///
/// [`Error::UnknownComponent`] when `named` is not one of the shell's
/// components.
pub(crate) fn component(
    program: &Path,
    named: Option<&str>,
) -> Result<Option<&'static Component>, Error> {
    if let Some(name) = named {
        return find(name).map(Some).ok_or_else(|| Error::UnknownComponent {
            name: name.to_owned(),
            known: COMPONENTS
                .iter()
                .map(|component| component.package)
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(program
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(find))
}

fn find(package: &str) -> Option<&'static Component> {
    COMPONENTS
        .iter()
        .find(|component| component.package == package)
}

/// The desktop file granting `executable` what `component` must be granted;
/// `None` when it uses nothing restricted.
pub(crate) fn entry(
    component: &Component,
    executable: &Path,
) -> Result<Option<DesktopEntry>, Error> {
    if component.wayland_grants.is_empty() {
        return Ok(None);
    }
    client(
        &format!("org.deskrinch.shell.{}", component.package),
        component.package,
        executable,
        component.wayland_grants,
        &[],
    )
    .map(Some)
}

/// The desktop file granting `executable`, a program that is not one of
/// the shell's components (a test binary, or xtask's own input client), the
/// restricted Wayland interfaces `wayland` and D-Bus interfaces `dbus`,
/// under the desktop id `id` and the name `name`.
pub(crate) fn client(
    id: &str,
    name: &str,
    executable: &Path,
    wayland: &[&str],
    dbus: &[&str],
) -> Result<DesktopEntry, Error> {
    Ok(DesktopEntry::new(id, name, executable, wayland, dbus)?)
}

/// Installs `entry` into the data directory `data_home`.
pub(crate) fn install_entry(entry: &DesktopEntry, data_home: &Path) -> Result<(), Error> {
    entry
        .install(data_home)
        .map(drop)
        .map_err(|source| Error::Io {
            what: format!("install {}", entry.file_name()),
            source,
        })
}

/// Installs [`entry`] into the data directory `data_home`; whether there
/// was anything to grant.
pub(crate) fn install(
    component: &Component,
    executable: &Path,
    data_home: &Path,
) -> Result<bool, Error> {
    let Some(entry) = entry(component, executable)? else {
        return Ok(false);
    };
    install_entry(&entry, data_home)?;
    Ok(true)
}

/// What granting `executable` as `component` grants, for a person to read.
pub(crate) fn describe(component: &Component, executable: &Path) -> String {
    format!(
        "{} to {}, as deskrinch-shell grants {}",
        component.wayland_grants.join(", "),
        executable.display(),
        component.package
    )
}

#[cfg(test)]
mod tests {
    use deskrinch_shell::{LAUNCHER, PANEL};

    use super::*;

    #[test]
    fn a_component_is_its_package_s_binary_or_named() {
        let panel = Path::new("/build/release/deskrinch-panel");
        assert_eq!(component(panel, None).unwrap(), Some(&PANEL));
        let renamed = Path::new("/tmp/measure/panel-after");
        assert_eq!(component(renamed, None).unwrap(), None);
        assert_eq!(
            component(renamed, Some("deskrinch-panel")).unwrap(),
            Some(&PANEL)
        );
        let error = component(renamed, Some("deskrinch-nothing")).unwrap_err();
        assert!(error.to_string().contains("deskrinch-panel"), "{error}");
    }

    #[test]
    fn grants_a_component_what_it_uses_and_nothing_to_one_that_uses_nothing() {
        let launcher = Path::new("/build/release/deskrinch-launcher");
        assert!(entry(&LAUNCHER, launcher).unwrap().is_none());
        let executable = Path::new("/build/release/deskrinch-panel");
        let panel = entry(&PANEL, executable).unwrap().unwrap();
        assert_eq!(
            panel.file_name(),
            "org.deskrinch.shell.deskrinch-panel.desktop"
        );
        let contents = panel.contents();
        assert!(
            contents.contains("Exec=/build/release/deskrinch-panel"),
            "{contents}"
        );
        for interface in PANEL.wayland_grants {
            assert!(contents.contains(interface), "{contents}");
        }
    }

    #[test]
    fn installs_a_grant_only_where_there_is_one() {
        let data = tempfile::tempdir().unwrap();
        let panel = Path::new("/build/release/deskrinch-panel");
        assert!(install(&PANEL, panel, data.path()).unwrap());
        assert!(
            data.path()
                .join("applications/org.deskrinch.shell.deskrinch-panel.desktop")
                .is_file()
        );
        let launcher = Path::new("/build/release/deskrinch-launcher");
        assert!(!install(&LAUNCHER, launcher, data.path()).unwrap());
        assert!(
            !data
                .path()
                .join("applications/org.deskrinch.shell.deskrinch-launcher.desktop")
                .exists()
        );
    }

    #[test]
    fn a_bare_name_is_found_on_the_path() {
        let sh = executable(OsStr::new("sh")).unwrap();
        assert!(sh.is_absolute() && sh.is_file(), "{}", sh.display());
    }

    #[test]
    fn a_grant_asked_for_is_a_component_and_its_executable() {
        let sh = executable(OsStr::new("sh")).unwrap();
        let asked = format!("deskrinch-panel={}", sh.display());
        assert_eq!(
            ComponentGrant::parse(&asked).unwrap(),
            ComponentGrant {
                component: &PANEL,
                executable: sh.clone(),
            }
        );
        let data = tempfile::tempdir().unwrap();
        assert!(
            ComponentGrant::parse(&asked)
                .unwrap()
                .install(data.path())
                .unwrap()
        );
        for refused in [
            "deskrinch-panel".to_owned(),
            format!("={}", sh.display()),
            "deskrinch-panel=".to_owned(),
            format!("deskrinch-nothing={}", sh.display()),
            "deskrinch-panel=/no/such/file".to_owned(),
        ] {
            assert!(ComponentGrant::parse(&refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_grant_is_described_with_its_executable() {
        assert_eq!(
            describe(&PANEL, Path::new("/tmp/measure/panel-after")),
            "org_kde_plasma_window_management to /tmp/measure/panel-after, as deskrinch-shell \
             grants deskrinch-panel"
        );
    }
}
