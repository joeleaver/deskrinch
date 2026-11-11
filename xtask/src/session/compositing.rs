//! What KWin composites with, as KWin itself reports it.
//!
//! KWin falls back to compositing with `QPainter` when its virtual backend
//! finds no DRM device to render on (a GPU-less machine without vgem). It
//! then runs, but `org.kde.KWin.ScreenShot2` captures nothing: KWin renders
//! screenshots with OpenGL. The session asks KWin's support information
//! (`org.kde.KWin.supportInformation` on `/KWin`, the text Plasma's "Copy
//! support information" shows), whose `Compositing Type:` line is KWin's
//! own statement, at every log level.

use std::time::Duration;

use super::Session;
use crate::error::Error;

/// The line of KWin's support information that names the compositing type.
const COMPOSITING_TYPE: &str = "Compositing Type: ";

/// Why a KWin compositing with anything but OpenGL is no use to a session.
const WITHOUT_OPENGL: &str = "KWin is not compositing with OpenGL, so it cannot take screenshots: \
     its virtual backend found no DRM device to render on (a GPU's, or vgem's with Mesa's \
     software driver)";

/// The compositing type KWin's support information `info` names.
fn compositing_type(info: &str) -> Option<&str> {
    info.lines()
        .find_map(|line| line.trim().strip_prefix(COMPOSITING_TYPE))
        .map(str::trim)
}

impl Session {
    /// Fails the session unless KWin says it composites with OpenGL.
    pub(super) fn check_kwin_compositing(&self, patience: Duration) -> Result<(), Error> {
        let info = support_information(self.bus.address(), patience).map_err(|source| {
            Error::CompositorQuery {
                compositor: self.compositor.name(),
                what: "KWin's support information",
                source: Box::new(source),
                log: self.log(),
            }
        })?;
        match compositing_type(&info) {
            Some("OpenGL") => Ok(()),
            other => Err(Error::CompositorUnfit {
                compositor: self.compositor.name(),
                why: WITHOUT_OPENGL,
                line: format!("{COMPOSITING_TYPE}{}", other.unwrap_or("(none)")),
                log: self.log(),
            }),
        }
    }
}

/// KWin's support information, from the bus at `address`.
fn support_information(address: &str, patience: Duration) -> zbus::Result<String> {
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(address)
            .method_timeout(patience)
            .build()
            .await?;
        let reply = connection
            .call_method(
                Some("org.kde.KWin"),
                "/KWin",
                Some("org.kde.KWin"),
                "supportInformation",
                &(),
            )
            .await?;
        reply.body().deserialize::<String>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As KWin 6.6 reports it, on a GPU and without one.
    const OPENGL: &str = "Compositing\n===========\nCompositing is active\nCompositing Type: OpenGL\nOpenGL vendor string: AMD\n";
    const QPAINTER: &str =
        "Compositing\n===========\nCompositing is active\nCompositing Type: QPainter\n";

    #[test]
    fn reads_the_compositing_type() {
        assert_eq!(compositing_type(OPENGL), Some("OpenGL"));
        assert_eq!(compositing_type(QPAINTER), Some("QPainter"));
        assert_eq!(compositing_type("Compositing is not active\n"), None);
    }
}
