//! The panel's settings: the `[panel]` table of deskrinch's configuration.
//!
//! ```toml
//! [panel]
//! edge = "bottom"    # "bottom" or "top": the output edge the panel runs along
//! height = 40        # logical pixels, 16 to 256
//!
//! [panel.clock]
//! seconds = false    # show the seconds, and change every second
//! date = false       # show the date before the time
//! ```
//!
//! Every key is optional; the values above are the defaults, which are the
//! panel as it was before it had settings. A value of the wrong type or out
//! of range (`height = 0`, `edge = "left"`) is a diagnostic with its file,
//! line and column, and that file's `[panel]` is not used: the panel keeps
//! the layers below it, or the file's last good version while it runs.
//!
//! The height is the panel's thickness and the space it reserves on its
//! edge. 16 is the least that holds a line of the panel's text; 256 is
//! already a quarter of a 1080-pixel-high output, past which a panel is no
//! longer a panel.

use std::fmt;

use deskrinch_clock::{Length, Precision, Style};
use deskrinch_config::{DefaultsError, Section};
use serde::{Deserialize, Serialize};

/// The configuration's table for the panel.
pub(crate) const TABLE: &str = "panel";

/// Every table of deskrinch's configuration, so that a misspelled one
/// (`[pannel]`) is reported. The list belongs to every component, and moves
/// to the planned shared settings crate when it exists; until then this is
/// its only copy.
pub(crate) const KNOWN_TABLES: &[&str] = &["general", "panel"];

/// The panel's settings, as `[panel]` writes them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PanelSettings {
    /// The edge of the output the panel runs along.
    pub(crate) edge: Edge,
    /// Its height, and the space it reserves on that edge.
    pub(crate) height: Height,
    /// What the clock shows.
    pub(crate) clock: ClockSettings,
}

impl PanelSettings {
    /// Where the panel is: what its window's surface is made of.
    pub(crate) fn placement(&self) -> Placement {
        Placement {
            edge: self.edge,
            height: self.height,
        }
    }
}

/// The edge of the output the panel runs along.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Edge {
    /// The bottom edge.
    #[default]
    Bottom,
    /// The top edge.
    Top,
}

/// The panel's height in logical pixels, within [`Height::RANGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub(crate) struct Height(u32);

impl Height {
    /// The heights a panel may have.
    pub(crate) const RANGE: std::ops::RangeInclusive<u32> = 16..=256;

    /// The height in logical pixels.
    pub(crate) const fn get(self) -> u32 {
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

/// A height outside [`Height::RANGE`]: the diagnostic's message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeightOutOfRange(u32);

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

/// What the clock shows: `[panel.clock]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ClockSettings {
    /// Show the seconds (and change every second), not only hours and
    /// minutes.
    pub(crate) seconds: bool,
    /// Show the date with the time.
    pub(crate) date: bool,
}

impl ClockSettings {
    /// How often the clock changes.
    pub(crate) fn precision(self) -> Precision {
        if self.seconds {
            Precision::Seconds
        } else {
            Precision::Minutes
        }
    }

    /// How the clock is written, in the user's locale.
    pub(crate) fn style(self) -> Style {
        if self.date {
            Style::DateTime(Length::Medium, self.precision())
        } else {
            Style::Time(self.precision())
        }
    }
}

/// Where the panel is: the settings its window's surface is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Placement {
    /// The edge it runs along.
    pub(crate) edge: Edge,
    /// Its height, which it also reserves.
    pub(crate) height: Height,
}

/// The panel's section of the configuration, reporting tables deskrinch
/// does not know.
pub(crate) fn section() -> Result<Section<PanelSettings>, DefaultsError> {
    Ok(Section::new(TABLE)?.with_known_tables(KNOWN_TABLES.iter().copied()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_config::{Problem, Severity, Snapshot};
    use std::path::PathBuf;

    /// The panel's settings from one user file holding `text`.
    fn resolve(text: &str) -> deskrinch_config::Resolved<PanelSettings> {
        let path = PathBuf::from("/home/u/.config/deskrinch/config.toml");
        let snapshot = Snapshot::from_texts([(path, Some(text.to_owned()))]);
        section().unwrap().resolve(&snapshot)
    }

    #[test]
    fn the_defaults_are_the_panel_before_it_had_settings() {
        let settings = resolve("").settings;
        assert_eq!(settings, PanelSettings::default());
        assert_eq!(settings.edge, Edge::Bottom);
        assert_eq!(settings.height.get(), 40);
        assert_eq!(settings.clock.precision(), Precision::Minutes);
        assert_eq!(settings.clock.style(), Style::Time(Precision::Minutes));
        assert!(section().is_ok());
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
            settings.clock.style(),
            Style::DateTime(Length::Medium, Precision::Seconds)
        );
        assert_eq!(
            settings.placement(),
            Placement {
                edge: Edge::Top,
                height: Height(56),
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
        let general = resolve("[general]\n").diagnostics;
        assert!(general.is_empty(), "{general:?}");
    }
}
