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

#[cfg(test)]
mod tests {
    use super::*;

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
