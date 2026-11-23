//! KWin's support information (`supportInformation` on `/KWin`, the text
//! Plasma's "Copy support information" shows): its version.

use crate::Error;
use crate::guard::{self, InKwinSession};

/// The line of the support information that names KWin's version.
const VERSION_LINE: &str = "KWin version: ";

/// KWin's version, as its support information names it (`6.6.7`).
///
/// # Errors
///
/// As [`windows`](crate::windows) outside an isolated KWin session, before
/// any connection; [`Error::Bus`] when KWin cannot be asked;
/// [`Error::NoVersion`] when its answer names none.
pub fn version() -> Result<String, Error> {
    guard::guarded(guard::require_kwin_here(), ask)
}

/// [`version`], once the session's check has passed.
fn ask(proof: &InKwinSession) -> Result<String, Error> {
    let information = zbus::block_on(async {
        let connection = guard::session_bus(proof).build().await?;
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
    })?;
    version_in(&information).ok_or(Error::NoVersion)
}

/// The version `information` names.
fn version_in(information: &str) -> Option<String> {
    information
        .lines()
        .find_map(|line| line.trim().strip_prefix(VERSION_LINE))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of what KWin 6.6.7 answered in `cargo xtask session kwin`
    /// (2026-11-23), its introduction shortened.
    const ANSWERED: &str = "KWin Support Information:\nThe following information should be used when requesting support on e.g. https://discuss.kde.org.\n\n==========================\n\nVersion\n=======\nKWin version: 6.6.7\nQt Version: 6.10.2\nQt compile version: 6.10.2\nXCB compile version: 1.17.0\n";

    #[test]
    fn the_version_is_read_from_its_line() {
        assert_eq!(version_in(ANSWERED).as_deref(), Some("6.6.7"));
        assert_eq!(version_in("Qt Version: 6.10.2\n"), None);
    }
}
