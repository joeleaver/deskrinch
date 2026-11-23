//! KWin's support information (`supportInformation` on `/KWin`, the text
//! Plasma's "Copy support information" shows): its version, and what it
//! composites with. One fetch; each line has a pure parser of its own.

use crate::{Error, Kwin};

/// The line of the support information that names KWin's version.
const VERSION_LINE: &str = "KWin version: ";

/// The line of the support information that names the compositing type.
const COMPOSITING_LINE: &str = "Compositing Type: ";

impl Kwin {
    /// KWin's version, as its support information names it (`6.6.7`).
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin cannot be asked; [`Error::NoVersion`] when
    /// its answer names none.
    pub fn version(&self) -> Result<String, Error> {
        version_in(&self.support_information()?)
            .map(str::to_owned)
            .ok_or(Error::NoVersion)
    }

    /// What KWin composites with, as its support information names it
    /// (`OpenGL`; `QPainter` when its virtual backend found no device to
    /// render on, in which case it takes no screenshots); `None` when it
    /// names none (it is not compositing).
    ///
    /// # Errors
    ///
    /// [`Error::Bus`] when KWin cannot be asked.
    pub fn compositing_type(&self) -> Result<Option<String>, Error> {
        Ok(compositing_type_in(&self.support_information()?).map(str::to_owned))
    }

    /// KWin's support information.
    fn support_information(&self) -> Result<String, Error> {
        zbus::block_on(async {
            let reply = self
                .connect()
                .await?
                .call_method(
                    Some("org.kde.KWin"),
                    "/KWin",
                    Some("org.kde.KWin"),
                    "supportInformation",
                    &(),
                )
                .await?;
            Ok(reply.body().deserialize::<String>()?)
        })
    }
}

/// The version `information` names.
fn version_in(information: &str) -> Option<&str> {
    line(information, VERSION_LINE)
}

/// The compositing type `information` names.
fn compositing_type_in(information: &str) -> Option<&str> {
    line(information, COMPOSITING_LINE)
}

/// What follows `prefix` on the line of `information` that starts with it.
fn line<'a>(information: &'a str, prefix: &str) -> Option<&'a str> {
    information
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What KWin 6.6.7 answered in `cargo xtask session kwin` (2026-11-23),
    /// the lines this module reads and those around them, the rest left
    /// out.
    const ANSWERED: &str = "KWin Support Information:\nThe following information should be used when requesting support on e.g. https://discuss.kde.org.\n\n==========================\n\nVersion\n=======\nKWin version: 6.6.7\nQt Version: 6.10.2\nQt compile version: 6.10.2\nXCB compile version: 1.17.0\n\nCompositing\n===========\nCompositing is active\nCompositing Type: OpenGL\n";

    #[test]
    fn the_version_is_read_from_its_line() {
        assert_eq!(version_in(ANSWERED), Some("6.6.7"));
        assert_eq!(version_in("Qt Version: 6.10.2\n"), None);
    }

    #[test]
    fn the_compositing_type_is_read_from_its_line() {
        assert_eq!(compositing_type_in(ANSWERED), Some("OpenGL"));
        assert_eq!(compositing_type_in("Compositing is not active\n"), None);
    }
}
