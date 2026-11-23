//! KWin's global shortcuts, as its shortcut service lists them
//! (`org.kde.kglobalaccel`, component `/component/kwin`).
//!
//! On a nested KWin's bus KWin itself owns the service's name: checked in
//! `cargo xtask session kwin` on KWin 6.6.7 (2026-11-23), the owner of
//! `org.kde.kglobalaccel` was `kwin_wayland`'s connection, the one that owns
//! `org.kde.KWin`. A shortcut KWin has not registered yet (an effect's,
//! before KWin has loaded it) is not listed.

use crate::names::SHORTCUTS;
use crate::{Error, Kwin};

impl Kwin {
    /// The names of KWin's global shortcuts its shortcut service lists
    /// (`shortcutNames` of the component `kwin`): `Overview`, say.
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when the service cannot be asked.
    pub fn kwin_shortcut_names(&self) -> Result<Vec<String>, Error> {
        zbus::block_on(async {
            let reply = self
                .connect()
                .await?
                .call_method(
                    Some(SHORTCUTS.0),
                    SHORTCUTS.1,
                    Some(SHORTCUTS.2),
                    "shortcutNames",
                    &(),
                )
                .await?;
            Ok(reply.body().deserialize::<Vec<String>>()?)
        })
    }
}
