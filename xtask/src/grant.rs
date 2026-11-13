//! What KWin grants the shell's components, from `deskrinch-shell`'s list
//! ([`Component::wayland_grants`]): KWin offers a restricted interface only
//! to an executable whose installed desktop file grants it
//! (`deskrinch-kwin-access`). Every xtask that starts a component under
//! KWin (`desktop`, `measure`) installs its grant through here, and
//! `compositor-tests` takes the panel's from the same list; sway and labwc
//! restrict nothing.

use std::path::Path;

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_shell::{COMPONENTS, Component};

use crate::error::Error;

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
    let entry = DesktopEntry::new(
        &format!("org.deskrinch.shell.{}", component.package),
        component.package,
        executable,
        component.wayland_grants,
        &[],
    )?;
    Ok(Some(entry))
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
    entry.install(data_home).map_err(|source| Error::Io {
        what: format!("install {}", entry.file_name()),
        source,
    })?;
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
    fn grants_each_component_what_it_uses_and_nothing_to_the_others() {
        let executable = Path::new("/build/release/deskrinch-launcher");
        assert!(entry(&LAUNCHER, executable).unwrap().is_none());
        let panel = entry(&PANEL, Path::new("/build/release/deskrinch-panel"))
            .unwrap()
            .unwrap();
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
        let executable = Path::new("/build/release/deskrinch-panel");
        assert!(install(&PANEL, executable, data.path()).unwrap());
        assert!(
            data.path()
                .join("applications/org.deskrinch.shell.deskrinch-panel.desktop")
                .is_file()
        );
        assert!(!install(&LAUNCHER, executable, data.path()).unwrap());
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
