//! `[panel]`: the panel along an edge of the output.
//!
//! ```toml
//! [panel]
//! edge = "bottom"    # "bottom" or "top": the output edge the panel runs along
//! height = 40        # logical pixels, 16 to 256
//!
//! [panel.clock]
//! seconds = false    # show the seconds, and change every second
//! date = false       # show the date with the time
//! ```
//!
//! Every key is optional; the values above are the defaults. A value of the
//! wrong type or out of range (`height = 0`, `edge = "left"`) is a
//! diagnostic with its file, line and column, and that file's `[panel]` is
//! not used: the panel keeps the layers below it, or the file's last good
//! version while it runs. A key the table does not have is a warning.
//!
//! - `edge`: the edge the panel runs along, the full width of the output.
//!   Windows keep clear of that edge.
//! - `height`: the panel's thickness and the space it reserves on its edge.
//!   16 is the least that holds a line of the panel's text; 256 is already
//!   a quarter of a 1080-pixel-high output, past which a panel is no longer
//!   a panel.
//! - `clock.seconds`: the clock shows hours, minutes and seconds, in the
//!   locale's format, and changes every second; otherwise hours and
//!   minutes, changing every minute.
//! - `clock.date`: the clock shows the date (day, abbreviated month, year,
//!   in the locale's order) before the time.

use std::fmt;
use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

/// The `[panel]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSettings {
    /// The edge of the output the panel runs along.
    pub edge: Edge,
    /// Its height, and the space it reserves on that edge.
    pub height: Height,
    /// What the clock shows: `[panel.clock]`.
    pub clock: ClockSettings,
}

/// `edge`: the edge of the output the panel runs along.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edge {
    /// `"bottom"`, the default.
    #[default]
    Bottom,
    /// `"top"`.
    Top,
}

/// `height`: the panel's height in logical pixels, within
/// [`Height::RANGE`]; 40 by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Height(u32);

impl Height {
    /// The heights a panel may have.
    pub const RANGE: RangeInclusive<u32> = 16..=256;

    /// The height in logical pixels.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for Height {
    fn default() -> Self {
        Self(40)
    }
}

impl TryFrom<u32> for Height {
    type Error = HeightOutOfRange;

    fn try_from(pixels: u32) -> Result<Self, Self::Error> {
        if Self::RANGE.contains(&pixels) {
            Ok(Self(pixels))
        } else {
            Err(HeightOutOfRange(pixels))
        }
    }
}

impl From<Height> for u32 {
    fn from(height: Height) -> Self {
        height.0
    }
}

/// A height outside [`Height::RANGE`]; its message is the diagnostic's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightOutOfRange(u32);

impl fmt::Display for HeightOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the height must be {} to {} logical pixels, not {}",
            Height::RANGE.start(),
            Height::RANGE.end(),
            self.0
        )
    }
}

impl std::error::Error for HeightOutOfRange {}

/// `[panel.clock]`: what the clock shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ClockSettings {
    /// `seconds`: show the seconds, and change every second; `false` by
    /// default.
    pub seconds: bool,
    /// `date`: show the date with the time; `false` by default.
    pub date: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section;
    use deskrinch_config::{Problem, Resolved, Severity, Snapshot};
    use std::path::PathBuf;

    /// The settings from one user file holding `text`.
    fn resolve(text: &str) -> Resolved<PanelSettings> {
        let path = PathBuf::from("/home/u/.config/deskrinch/config.toml");
        let snapshot = Snapshot::from_texts([(path, Some(text.to_owned()))]);
        section::<PanelSettings>().unwrap().resolve(&snapshot)
    }

    #[test]
    fn the_defaults() {
        let settings = resolve("").settings;
        assert_eq!(settings, PanelSettings::default());
        assert_eq!(settings.edge, Edge::Bottom);
        assert_eq!(settings.height.get(), 40);
        assert!(!settings.clock.seconds && !settings.clock.date);
    }

    #[test]
    fn reads_every_setting() {
        let resolved = resolve(
            "[panel]\nedge = \"top\"\nheight = 56\n[panel.clock]\nseconds = true\ndate = true\n",
        );
        assert!(
            resolved.diagnostics.is_empty(),
            "{:?}",
            resolved.diagnostics
        );
        let settings = resolved.settings;
        assert_eq!(settings.edge, Edge::Top);
        assert_eq!(settings.height.get(), 56);
        assert_eq!(
            settings.clock,
            ClockSettings {
                seconds: true,
                date: true
            }
        );
    }

    #[test]
    fn a_partial_table_keeps_the_other_defaults() {
        let settings = resolve("[panel.clock]\nseconds = true\n").settings;
        assert_eq!(settings.edge, Edge::Bottom);
        assert_eq!(settings.height, Height::default());
        assert!(settings.clock.seconds && !settings.clock.date);
    }

    #[test]
    fn rejects_heights_out_of_range_with_their_location() {
        for (text, value) in [
            ("[panel]\nheight = 0\n", 0),
            ("[panel]\nheight = 10000\n", 10_000),
        ] {
            let resolved = resolve(text);
            assert_eq!(resolved.settings, PanelSettings::default(), "{text}");
            let [diagnostic] = resolved.diagnostics.as_slice() else {
                panic!("{:?}", resolved.diagnostics);
            };
            assert_eq!(diagnostic.severity(), Severity::Error);
            assert_eq!(diagnostic.location.map(|l| l.line), Some(2));
            let message = diagnostic.to_string();
            assert!(
                message.contains(&format!("16 to 256 logical pixels, not {value}")),
                "{message}"
            );
        }
        assert!(Height::try_from(16).is_ok() && Height::try_from(256).is_ok());
        assert!(Height::try_from(15).is_err() && Height::try_from(257).is_err());
    }

    #[test]
    fn rejects_nonsense_values() {
        for text in [
            "[panel]\nedge = \"left\"\n",
            "[panel]\nheight = -4\n",
            "[panel]\nheight = \"tall\"\n",
            "[panel.clock]\nseconds = \"yes\"\n",
        ] {
            let resolved = resolve(text);
            assert_eq!(resolved.settings, PanelSettings::default(), "{text}");
            assert!(
                resolved
                    .diagnostics
                    .iter()
                    .any(|d| matches!(d.problem, Problem::Invalid { .. })),
                "{text}: {:?}",
                resolved.diagnostics
            );
        }
    }

    #[test]
    fn reports_unknown_keys_and_tables() {
        let resolved = resolve("[pannel]\nheight = 50\n[panel]\nhieght = 50\n");
        assert_eq!(resolved.settings, PanelSettings::default());
        let problems: Vec<_> = resolved.diagnostics.iter().map(|d| &d.problem).collect();
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::UnknownTable { table } if table == "pannel")),
            "{problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::UnknownKey { .. })),
            "{problems:?}"
        );
    }
}
