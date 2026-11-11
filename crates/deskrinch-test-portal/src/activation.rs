//! A private bus that starts the stand-in when a client asks for it.

use std::path::Path;

use deskrinch_test_dbus::PrivateBus;

use crate::{Error, NAME, SETTINGS_FILE, Setting, Value};

/// A private bus with a service file for the portal: asked for it
/// (`StartServiceByName`, or a call to its name), the bus runs `program`
/// with `args` as the stand-in, which must call [`serve_from_env`]
/// (`crate::serve_from_env`). Ends with the bus, which ends its process
/// group, the stand-in included.
///
/// [`serve_from_env`]: crate::serve_from_env
#[derive(Debug)]
pub struct ActivatingBus {
    bus: PrivateBus,
    _files: tempfile::TempDir,
}

impl ActivatingBus {
    /// A bus whose activated stand-in answers `settings`. A test usually
    /// passes its own binary for `program`, with arguments that run one
    /// ignored test calling [`serve_from_env`] (`crate::serve_from_env`).
    ///
    /// # Errors
    ///
    /// When the settings cannot be written as a settings file, a file
    /// cannot be written, or the bus cannot start.
    ///
    /// [`serve_from_env`]: crate::serve_from_env
    pub fn start(
        settings: &[(Setting<'_>, Value)],
        program: &Path,
        args: &[&str],
    ) -> Result<Self, Error> {
        let files = tempfile::tempdir().map_err(Error::Write)?;
        let settings_file = files.path().join("settings");
        let text = crate::format(settings.iter().map(|(setting, value)| (*setting, value)))?;
        std::fs::write(&settings_file, text).map_err(Error::Write)?;
        // Through `sh` to give the program a standard output (the daemon's
        // is a pipe nobody reads) and the settings file to answer from.
        let args = args.iter().fold(String::new(), |mut all, arg| {
            all.push(' ');
            all.push_str(arg);
            all
        });
        let text = format!(
            "[D-BUS Service]\nName={NAME}\n\
             Exec=/bin/sh -c '{SETTINGS_FILE}=\"$1\" exec \"$0\"{args} >/dev/null' {} {}\n",
            program.display(),
            settings_file.display(),
        );
        std::fs::write(files.path().join(format!("{NAME}.service")), text).map_err(Error::Write)?;
        let bus = PrivateBus::start_with_services(&[files.path()])?;
        Ok(Self { bus, _files: files })
    }

    /// The bus.
    #[must_use]
    pub fn bus(&self) -> &PrivateBus {
        &self.bus
    }
}
