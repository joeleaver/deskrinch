//! The restricted interfaces the shell's components use, granted to them
//! under KWin before they start: KWin offers such an interface only to an
//! executable whose installed desktop file grants it
//! (`deskrinch-kwin-access`). Without it, the panel's taskbar would find no
//! window list and stay empty. sway and labwc restrict none of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deskrinch_kwin_access::DesktopEntry;
use deskrinch_shell::{COMPONENTS, Component};

use crate::error::Error;

/// Installs into the session's data directory `data_home`, for each
/// component that uses a restricted interface ([`Component::wayland_grants`]),
/// the desktop file that grants it to the component's binary in `binaries`.
/// The file is written anew at each start, for the binary just built.
pub(super) fn install(
    binaries: &BTreeMap<&'static str, PathBuf>,
    data_home: &Path,
) -> Result<(), Error> {
    for component in COMPONENTS {
        let Some(entry) = entry(component, binaries)? else {
            continue;
        };
        entry.install(data_home).map_err(|source| Error::Io {
            what: format!("install {}", entry.file_name()),
            source,
        })?;
    }
    Ok(())
}

/// The desktop file granting `component`'s binary its restricted
/// interfaces; `None` when it uses none.
fn entry(
    component: &Component,
    binaries: &BTreeMap<&'static str, PathBuf>,
) -> Result<Option<DesktopEntry>, Error> {
    if component.wayland_grants.is_empty() {
        return Ok(None);
    }
    let binary = super::binary(binaries, component.package)?;
    let entry = DesktopEntry::new(
        &format!("org.deskrinch.shell.{}", component.package),
        component.package,
        binary,
        component.wayland_grants,
        &[],
    )?;
    Ok(Some(entry))
}

#[cfg(test)]
mod tests {
    use deskrinch_shell::{LAUNCHER, PANEL};

    use super::*;

    #[test]
    fn grants_each_component_what_it_uses_and_nothing_to_the_others() {
        let binaries: BTreeMap<&'static str, PathBuf> = COMPONENTS
            .iter()
            .map(|component| {
                let path = PathBuf::from(format!("/build/release/{}", component.package));
                (component.package, path)
            })
            .collect();
        assert!(entry(&LAUNCHER, &binaries).unwrap().is_none());
        let panel = entry(&PANEL, &binaries).unwrap().unwrap();
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
    fn a_component_without_its_binary_is_an_error() {
        let binaries = BTreeMap::new();
        assert!(entry(&PANEL, &binaries).is_err());
    }
}
