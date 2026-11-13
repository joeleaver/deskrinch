//! The restricted interfaces the shell's components use, granted to them
//! under KWin before they start ([`crate::grant`]). Without them, the
//! panel's taskbar would find no window list and stay empty.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deskrinch_shell::COMPONENTS;

use crate::error::Error;

/// Installs into the session's data directory `data_home`, for each
/// component that uses a restricted interface, the desktop file that grants
/// it to the component's binary in `binaries`. The file is written anew at
/// each start, for the binary just built.
pub(super) fn install(
    binaries: &BTreeMap<&'static str, PathBuf>,
    data_home: &Path,
) -> Result<(), Error> {
    for component in COMPONENTS {
        let binary = super::binary(binaries, component.package)?;
        crate::grant::install(component, binary, data_home)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_the_built_components_and_needs_each_binary() {
        let data = tempfile::tempdir().unwrap();
        let binaries: BTreeMap<&'static str, PathBuf> = COMPONENTS
            .iter()
            .map(|component| {
                let path = PathBuf::from(format!("/build/release/{}", component.package));
                (component.package, path)
            })
            .collect();
        install(&binaries, data.path()).unwrap();
        let installed = data
            .path()
            .join("applications/org.deskrinch.shell.deskrinch-panel.desktop");
        assert!(installed.is_file());
        assert!(install(&BTreeMap::new(), data.path()).is_err());
    }
}
