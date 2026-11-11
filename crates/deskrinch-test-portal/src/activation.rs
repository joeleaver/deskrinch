//! A private bus that starts the stand-in when a client asks for it.

use std::path::Path;

use deskrinch_test_dbus::PrivateBus;

use crate::{Error, NAME, SCHEME_VALUE};

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
    _services: tempfile::TempDir,
}

impl ActivatingBus {
    /// A bus whose activated stand-in answers `scheme` (a colour-scheme
    /// number). A test usually passes its own binary for `program`, with
    /// arguments that run one ignored test calling [`serve_from_env`]
    /// (`crate::serve_from_env`).
    ///
    /// # Errors
    ///
    /// When the service file cannot be written or the bus cannot start.
    ///
    /// [`serve_from_env`]: crate::serve_from_env
    pub fn start(scheme: u32, program: &Path, args: &[&str]) -> Result<Self, Error> {
        let services = tempfile::tempdir().map_err(Error::ServiceFile)?;
        // Through `sh` to give the program a standard output (the daemon's
        // is a pipe nobody reads) and the scheme to answer.
        let args = args.iter().fold(String::new(), |mut all, arg| {
            all.push(' ');
            all.push_str(arg);
            all
        });
        let text = format!(
            "[D-BUS Service]\nName={NAME}\n\
             Exec=/bin/sh -c '{SCHEME_VALUE}={scheme} exec \"$0\"{args} >/dev/null' {}\n",
            program.display()
        );
        std::fs::write(services.path().join(format!("{NAME}.service")), text)
            .map_err(Error::ServiceFile)?;
        let bus = PrivateBus::start_with_services(&[services.path()])?;
        Ok(Self {
            bus,
            _services: services,
        })
    }

    /// The bus.
    #[must_use]
    pub fn bus(&self) -> &PrivateBus {
        &self.bus
    }
}
