//! `[wallpaper]`: what the background of every output shows.
//!
//! ```toml
//! [wallpaper]
//! color = "#1e2430"                     # unset: the look's background colour
//! image = "/home/me/Pictures/hills.jpg" # unset: the colour alone
//! fit = "cover"                         # cover, contain, stretch, center or tile
//! ```
//!
//! Every key is optional; `fit = "cover"` is the default, and with neither
//! `color` nor `image` the wallpaper is the look's background colour, dark
//! or light as `[general]` `color-scheme` says. A value of the wrong type
//! or not one of those allowed is a diagnostic with its file, line and
//! column, and that file's `[wallpaper]` is not used: the wallpaper keeps
//! the layers below it, or the file's last good version while it runs. A
//! key the table does not have is a warning.
//!
//! - `color`: the colour, `#rrggbb`, shown where the image is not (all of
//!   the output without one; around it with `contain` and `center`; under
//!   a transparent one).
//! - `image`: the picture: an absolute path, or one starting `~/` for the
//!   home directory. PNG, JPEG, GIF and WebP. One that is missing or cannot
//!   be read or decoded leaves the colour alone, and the wallpaper says so,
//!   naming the path it looked at.
//! - `fit`: how the image meets an output whose shape is not its own:
//!   - `cover`: scaled to fill the output, cropped to it, centred;
//!   - `contain`: scaled to fit inside the output whole, centred, with the
//!     colour around it;
//!   - `stretch`: scaled to the output's size, its shape lost;
//!   - `center`: at its own size, centred, cropped or with the colour
//!     around it;
//!   - `tile`: at its own size, repeated from the top-left corner.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The `[wallpaper]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WallpaperSettings {
    /// `color`: the colour, or the look's background colour when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// `image`: the picture, or none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImagePath>,
    /// `fit`: how the picture meets the output.
    pub fit: Fit,
}

/// `color`: an opaque colour, written `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color {
    /// Red, 0 to 255.
    pub red: u8,
    /// Green, 0 to 255.
    pub green: u8,
    /// Blue, 0 to 255.
    pub blue: u8,
}

impl fmt::Display for Color {
    /// `#rrggbb`, in lower case: as CSS reads it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

impl TryFrom<String> for Color {
    type Error = NotAColor;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let digits = text
            .strip_prefix('#')
            .filter(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| NotAColor(text.clone()))?;
        let channel = |at: usize| {
            u8::from_str_radix(&digits[at..at + 2], 16).map_err(|_| NotAColor(text.clone()))
        };
        Ok(Self {
            red: channel(0)?,
            green: channel(2)?,
            blue: channel(4)?,
        })
    }
}

impl From<Color> for String {
    fn from(color: Color) -> Self {
        color.to_string()
    }
}

/// A `color` that is not `#rrggbb`; its message is the diagnostic's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAColor(String);

impl fmt::Display for NotAColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the colour must be written #rrggbb, six hexadecimal digits, not {:?}",
            self.0
        )
    }
}

impl std::error::Error for NotAColor {}

/// `image`: the picture's path as written, absolute or under `~/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ImagePath(String);

impl ImagePath {
    /// The path as written.
    #[must_use]
    pub fn as_written(&self) -> &str {
        &self.0
    }

    /// The path itself, with a leading `~/` taken as `home`; `None` for a
    /// `~/` path when there is no home directory.
    #[must_use]
    pub fn expand(&self, home: Option<&Path>) -> Option<PathBuf> {
        match self.0.strip_prefix("~/") {
            Some(rest) => home.map(|home| home.join(rest)),
            None => Some(PathBuf::from(&self.0)),
        }
    }
}

impl TryFrom<String> for ImagePath {
    type Error = NotAnImagePath;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let under_home = text.strip_prefix("~/").is_some_and(|rest| !rest.is_empty());
        if under_home || Path::new(&text).is_absolute() {
            Ok(Self(text))
        } else {
            Err(NotAnImagePath(text))
        }
    }
}

impl From<ImagePath> for String {
    fn from(path: ImagePath) -> Self {
        path.0
    }
}

/// An `image` neither absolute nor under `~/`; its message is the
/// diagnostic's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAnImagePath(String);

impl fmt::Display for NotAnImagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("the image path is empty; leave `image` out for the colour alone")
        } else {
            write!(
                f,
                "the image path must be absolute or start with ~/, not {:?}",
                self.0
            )
        }
    }
}

impl std::error::Error for NotAnImagePath {}

/// `fit`: how the picture meets an output whose shape is not its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// `"cover"`, the default: fills the output, cropped, centred.
    #[default]
    Cover,
    /// `"contain"`: inside the output whole, centred, the colour around.
    Contain,
    /// `"stretch"`: the output's size, its shape lost.
    Stretch,
    /// `"center"`: its own size, centred.
    Center,
    /// `"tile"`: its own size, repeated from the top-left corner.
    Tile,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section;
    use deskrinch_config::{Resolved, Severity, Snapshot};

    /// The settings from one user file holding `text`.
    fn resolve(text: &str) -> Resolved<WallpaperSettings> {
        let path = PathBuf::from("/home/u/.config/deskrinch/config.toml");
        let snapshot = Snapshot::from_texts([(path, Some(text.to_owned()))]);
        section::<WallpaperSettings>().unwrap().resolve(&snapshot)
    }

    /// The one error `text` gives, and that the defaults stayed.
    fn error(text: &str) -> String {
        let resolved = resolve(text);
        assert_eq!(resolved.settings, WallpaperSettings::default(), "{text}");
        let [diagnostic] = resolved.diagnostics.as_slice() else {
            panic!("{:?}", resolved.diagnostics);
        };
        assert_eq!(diagnostic.severity(), Severity::Error);
        assert_eq!(diagnostic.location.map(|l| l.line), Some(2));
        diagnostic.to_string()
    }

    #[test]
    fn the_defaults_are_the_looks_colour_alone() {
        let settings = resolve("").settings;
        assert_eq!(settings.color, None);
        assert_eq!(settings.image, None);
        assert_eq!(settings.fit, Fit::Cover);
    }

    #[test]
    fn reads_every_setting() {
        let resolved = resolve(
            "[wallpaper]\ncolor = \"#1E2430\"\nimage = \"~/Pictures/a.png\"\nfit = \"tile\"\n",
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let settings = resolved.settings;
        assert_eq!(
            settings.color,
            Some(Color {
                red: 0x1e,
                green: 0x24,
                blue: 0x30
            })
        );
        assert_eq!(settings.color.unwrap().to_string(), "#1e2430");
        let image = settings.image.unwrap();
        assert_eq!(image.as_written(), "~/Pictures/a.png");
        assert_eq!(
            image.expand(Some(Path::new("/home/u"))),
            Some(PathBuf::from("/home/u/Pictures/a.png"))
        );
        assert_eq!(image.expand(None), None);
        assert_eq!(settings.fit, Fit::Tile);
        for (written, fit) in [
            ("cover", Fit::Cover),
            ("contain", Fit::Contain),
            ("stretch", Fit::Stretch),
            ("center", Fit::Center),
            ("tile", Fit::Tile),
        ] {
            let text = format!("[wallpaper]\nfit = \"{written}\"\n");
            assert_eq!(resolve(&text).settings.fit, fit);
        }
    }

    #[test]
    fn an_absolute_path_is_itself() {
        let path = ImagePath::try_from("/srv/a.jpg".to_owned()).unwrap();
        assert_eq!(path.expand(None), Some(PathBuf::from("/srv/a.jpg")));
    }

    #[test]
    fn rejects_what_is_not_a_colour_with_its_location() {
        for written in ["red", "#12345", "#1234567", "#gggggg", "1e2430"] {
            let message = error(&format!("[wallpaper]\ncolor = \"{written}\"\n"));
            assert!(message.contains("#rrggbb"), "{message}");
        }
    }

    #[test]
    fn rejects_a_relative_or_empty_image_path() {
        let relative = error("[wallpaper]\nimage = \"Pictures/a.png\"\n");
        assert!(relative.contains("absolute or start with ~/"), "{relative}");
        let empty = error("[wallpaper]\nimage = \"\"\n");
        assert!(empty.contains("leave `image` out"), "{empty}");
        let home_alone = error("[wallpaper]\nimage = \"~/\"\n");
        assert!(
            home_alone.contains("absolute or start with ~/"),
            "{home_alone}"
        );
    }

    #[test]
    fn rejects_an_unknown_fit() {
        let message = error("[wallpaper]\nfit = \"zoom\"\n");
        assert!(message.contains("zoom"), "{message}");
    }
}
