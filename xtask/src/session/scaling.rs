//! Putting a session's outputs at the scale asked for, before its first
//! client, and checking that they are.

use std::ffi::OsStr;

use deskrinch_test_client::{OutputChange, TestClient, configure_every_output};

use super::{Scale, Session};
use crate::error::Error;

/// How a compositor's outputs get their scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scaling {
    /// From its start-up configuration (sway's `output * scale`).
    StartUp,
    /// Set by the session through `wlr-output-management` once it runs
    /// (labwc, which has no start-up setting for a headless output's scale).
    OutputManagement,
    /// Set by the session with KDE's `kscreen-doctor` once it runs (KWin:
    /// its `--scale` enlarges the virtual outputs' mode instead, and its
    /// own output protocol is not usable from Rust here).
    KscreenDoctor,
}

/// KDE's command-line tool for output settings.
const KSCREEN_DOCTOR: &str = "kscreen-doctor";

impl Session {
    /// Puts every output at `scale`, where the compositor did not start at
    /// it, then checks the scale every output reports.
    pub(super) fn scale_outputs(&self, scale: Scale) -> Result<(), Error> {
        if !scale.is_one() {
            match self.compositor.scaling() {
                Scaling::StartUp => {}
                Scaling::OutputManagement => self.scale_through_output_management(scale)?,
                Scaling::KscreenDoctor => self.scale_through_kscreen_doctor(scale)?,
            }
        }
        self.check_scale(scale)
    }

    /// The path of the compositor's Wayland socket.
    fn socket_path(&self) -> std::path::PathBuf {
        self.root.join("run").join(&self.socket)
    }

    /// Sets every output to `scale` through `wlr-output-management`, the way
    /// a test's `configure_output` does, and returns once it applies.
    fn scale_through_output_management(&self, scale: Scale) -> Result<(), Error> {
        let change = OutputChange {
            scale: Some(scale.get()),
            transform: None,
        };
        configure_every_output(&self.socket_path(), change).map_err(|source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: format!("set the outputs' scale to {scale}"),
            source: Box::new(source),
            log: self.log(),
        })
    }

    /// Sets every output to `scale` with `kscreen-doctor`, run as a client
    /// of the session like any other (its environment, its process group).
    fn scale_through_kscreen_doctor(&self, scale: Scale) -> Result<(), Error> {
        let outputs = self.connect_client()?.output_names();
        let output = self
            .client(OsStr::new(KSCREEN_DOCTOR))
            // Qt 5 picks X11 by default, and there is no X server here;
            // Qt 6 finds the session's Wayland on its own.
            .env("QT_QPA_PLATFORM", "wayland")
            .args(
                outputs
                    .iter()
                    .map(|name| format!("output.{name}.scale.{scale}")),
            )
            .output()
            .map_err(|source| Error::Spawn {
                program: KSCREEN_DOCTOR.into(),
                source,
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::ScaleToolFailed {
                program: KSCREEN_DOCTOR,
                status: output.status,
                output: String::from_utf8_lossy(&[output.stdout, output.stderr].concat())
                    .into_owned(),
                log: self.log(),
            })
        }
    }

    /// Checks that every output reports `scale`, as `wl_output` reports it
    /// (a whole number, rounded up), so that a compositor that ignored the
    /// setting fails the session instead of running at another scale.
    fn check_scale(&self, scale: Scale) -> Result<(), Error> {
        let client = self.connect_client()?;
        let wrong: Vec<String> = client
            .output_names()
            .into_iter()
            .filter_map(|name| {
                let reported = client.output_scale(&name);
                (reported != Some(scale.whole())).then(|| match reported {
                    Some(reported) => format!("{name} reports {reported}"),
                    None => format!("{name} reports none"),
                })
            })
            .collect();
        if wrong.is_empty() {
            Ok(())
        } else {
            Err(Error::WrongScale {
                compositor: self.compositor.name(),
                asked: scale.to_string(),
                whole: scale.whole(),
                outputs: wrong.join(", "),
                log: self.log(),
            })
        }
    }

    /// A Wayland client of the session, for setting it up.
    fn connect_client(&self) -> Result<TestClient, Error> {
        TestClient::connect_to(&self.socket_path()).map_err(|source| Error::OutputSetup {
            compositor: self.compositor.name(),
            what: "list the outputs".into(),
            source: Box::new(source),
            log: self.log(),
        })
    }
}
