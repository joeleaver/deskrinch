//! What the `deskrinch-panel` binary says, with one home for its wording:
//! the panel logs with these functions, and its tests wait with them.
//!
//! The binary is the panel itself (`src/main.rs`); this library holds no
//! more of it than what both the binary and its tests read.

use deskrinch_settings::panel::{Edge, Height};

/// The line the panel writes, at info, each time its main thread applies a
/// new edge and height to the bar on `output` (the output's name, or its
/// id where it has none): once per bar per applied change of either, never
/// per frame. It says where that bar now is, which is worth having in a
/// journal; and since only the main thread writes it, a test sees from it
/// that the main thread goes on working while the compositor does not show
/// the panel.
#[must_use]
pub fn placed(output: &str, edge: Edge, height: Height) -> String {
    let edge = match edge {
        Edge::Bottom => "bottom",
        Edge::Top => "top",
    };
    format!(
        "the bar on {output} is placed along the {edge} edge, {} high",
        height.get()
    )
}

/// What every [`clock_shows`] line starts with, for a test that looks for
/// any of them.
pub const CLOCK_SHOWS: &str = "the clock shows ";

/// The line the panel writes, at debug, each time its main thread applies a
/// new text of the clock, `text`: before the frame that draws it on every
/// bar. At debug, since a line a minute for a session's life is nothing a
/// journal needs; a test that judges a bar's repaints turns it on (for the
/// clock's module alone) to tell the clock's frames from others by the
/// panel's own word, not by the time.
#[must_use]
pub fn clock_shows(text: &str) -> String {
    format!("{CLOCK_SHOWS}{text:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_the_clock_shows() {
        assert_eq!(clock_shows("14:05"), "the clock shows \"14:05\"");
        assert!(clock_shows("").starts_with(CLOCK_SHOWS));
    }

    #[test]
    fn says_the_bar_the_edge_and_the_height() {
        let height = Height::try_from(56).unwrap();
        assert_eq!(
            placed("DP-1", Edge::Bottom, height),
            "the bar on DP-1 is placed along the bottom edge, 56 high"
        );
        assert_eq!(
            placed("HDMI-A-1", Edge::Top, height),
            "the bar on HDMI-A-1 is placed along the top edge, 56 high"
        );
    }
}
