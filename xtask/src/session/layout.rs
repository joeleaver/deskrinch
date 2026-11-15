//! The session's outputs as its compositor reports them, read from
//! outside: for a runner to check that a test binary left them as it found
//! them.
//!
//! The reading is `deskrinch-test-client`'s, one home: through
//! `wlr-output-management` on the compositor's socket under sway and labwc,
//! and `kscreen-doctor -j`, run as a client of the session, under KWin.

use std::ffi::OsStr;

use deskrinch_test_client::{KSCREEN_DOCTOR, OutputState, kscreen_outputs, wlroots_outputs_at};

use super::{Compositor, Session};
use crate::error::Error;

impl Session {
    /// Every output of the session, on or off, as the compositor reports
    /// it.
    pub(crate) fn output_layout(&self) -> Result<Vec<OutputState>, Error> {
        let failed = |source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: "list the outputs".to_owned(),
            source: Box::new(source),
            log: self.log(),
        };
        match self.compositor {
            Compositor::Sway | Compositor::Labwc => {
                wlroots_outputs_at(&self.socket_path()).map_err(failed)
            }
            Compositor::Kwin => {
                let output = self
                    .client(OsStr::new(KSCREEN_DOCTOR))
                    // Qt 5 picks X11 by default, and there is no X server
                    // here; Qt 6 finds the session's Wayland on its own.
                    .env("QT_QPA_PLATFORM", "wayland")
                    .arg("-j")
                    .output()
                    .map_err(|source| Error::Spawn {
                        program: KSCREEN_DOCTOR.into(),
                        source,
                    })?;
                if !output.status.success() {
                    return Err(Error::ScaleToolFailed {
                        program: KSCREEN_DOCTOR,
                        status: output.status,
                        output: String::from_utf8_lossy(&[output.stdout, output.stderr].concat())
                            .into_owned(),
                        log: self.log(),
                    });
                }
                kscreen_outputs(&String::from_utf8_lossy(&output.stdout)).map_err(failed)
            }
        }
    }
}
