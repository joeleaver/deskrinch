//! `--window`: one window opened in the measured program's session at the
//! idle window's start, and kept open to its end, so that the idle
//! window's figures include what that window costs the program (a panel's
//! taskbar following it, say).
//!
//! The window is a `deskrinch-test-client` window with the app id the
//! caller names: what a component does with a window depends on whether
//! that id matches an installed application (an icon is found and drawn,
//! or not), so the id is part of what is measured and the report names it.
//! The window counts as opened only once it has had its first configure
//! and committed a buffer, and it must still be open at the end of the
//! idle window; otherwise the run fails, and no report is printed for it.
//!
//! A run with `--window` minus one without is the cost of a window
//! appearing plus whatever a window held open costs over the window; the
//! idle window's totals of wake-ups and frames, which the report then
//! prints, tell the two apart.

use std::path::PathBuf;

use deskrinch_test_client::{Paint, TestClient, TestWindow};
use deskrinch_test_wayland::Rgba;

use super::report::Run;
use crate::error::Error;
use crate::session::Session;

/// What the idle window starts with.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stimulus {
    /// `--window`: the session's Wayland socket, and the app id.
    window: Option<(PathBuf, String)>,
}

impl Stimulus {
    /// Nothing: the idle window is idle.
    pub(crate) fn none() -> Self {
        Self::default()
    }

    /// A window with `app_id`, when there is one, opened through `session`'s
    /// compositor.
    pub(crate) fn of(session: &Session, app_id: Option<&str>) -> Self {
        Self {
            window: app_id.map(|app_id| (session.socket_path(), app_id.to_owned())),
        }
    }

    /// Whether a window is opened: the report then prints totals as well as
    /// rates.
    pub(crate) fn opens_a_window(&self) -> bool {
        self.window.is_some()
    }

    /// At the idle window's start: opens the window, if one is asked for,
    /// and returns once it has drawn its first buffer.
    pub(crate) fn begin(&self) -> Result<Option<Opened>, Error> {
        let Some((socket, app_id)) = &self.window else {
            return Ok(None);
        };
        let failed = |source| Error::WindowNotOpened {
            app_id: app_id.clone(),
            source: Box::new(source),
        };
        let client = TestClient::connect_to(socket).map_err(failed)?;
        let window = client
            .open_painted(
                "A measured window",
                app_id,
                Paint::Solid(Rgba::opaque(60, 110, 170)),
                None,
            )
            .map_err(failed)?;
        Ok(Some(Opened {
            app_id: app_id.clone(),
            client,
            window,
        }))
    }
}

/// The window, open.
#[derive(Debug)]
pub(crate) struct Opened {
    app_id: String,
    client: TestClient,
    window: TestWindow,
}

/// At the idle window's end: checks that the window, if one was opened, is
/// still open, and notes it in `run`.
pub(crate) fn end(opened: Option<Opened>, run: &mut Run) -> Result<(), Error> {
    let Some(opened) = opened else {
        return Ok(());
    };
    let gone = |why| Error::WindowGone {
        app_id: opened.app_id.clone(),
        why,
    };
    if !opened.client.is_connected() {
        return Err(gone("its connection to the compositor ended"));
    }
    if opened.window.close_requested() {
        return Err(gone("the compositor asked it to close"));
    }
    run.note(
        "window",
        format!(
            "a window with app id {} opened at the idle window's start",
            opened.app_id
        ),
    );
    Ok(())
}
