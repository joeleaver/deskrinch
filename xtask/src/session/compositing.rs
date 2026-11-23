//! What KWin composites with, as KWin itself reports it.
//!
//! KWin falls back to compositing with `QPainter` when its virtual backend
//! finds no DRM device to render on (a GPU-less machine without vgem). It
//! then runs, but `org.kde.KWin.ScreenShot2` captures nothing: KWin renders
//! screenshots with OpenGL. The session asks KWin
//! ([`Kwin::compositing_type`], its support information's own statement,
//! at every log level) on the bus of the session it started, which is a
//! private bus of ours ([`Kwin::on`]).

use std::time::Duration;

use deskrinch_test_kwin::Kwin;

use super::Session;
use crate::error::Error;

/// Why a KWin compositing with anything but OpenGL is no use to a session.
const WITHOUT_OPENGL: &str = "KWin is not compositing with OpenGL, so it cannot take screenshots: \
     its virtual backend found no DRM device to render on (a GPU's, or vgem's with Mesa's \
     software driver)";

impl Session {
    /// Fails the session unless KWin says it composites with OpenGL.
    pub(super) fn check_kwin_compositing(&self, patience: Duration) -> Result<(), Error> {
        let compositing = Kwin::on(self.bus.address())
            .and_then(|kwin| kwin.patience(patience).compositing_type())
            .map_err(|source| Error::CompositorQuery {
                compositor: self.compositor.name(),
                what: "KWin's compositing type",
                source: Box::new(source),
                log: self.log(),
            })?;
        match compositing.as_deref() {
            Some("OpenGL") => Ok(()),
            other => Err(Error::CompositorUnfit {
                compositor: self.compositor.name(),
                why: WITHOUT_OPENGL,
                line: format!("Compositing Type: {}", other.unwrap_or("(none)")),
                log: self.log(),
            }),
        }
    }
}
