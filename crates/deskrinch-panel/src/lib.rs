//! What the `deskrinch-panel` binary says, with one home for its wording:
//! the panel logs with these functions, and its tests wait with them.
//!
//! The binary is the panel itself (`src/main.rs`); this library holds no
//! more of it than what both the binary and its tests read.

use deskrinch_settings::panel::{Edge, Height};

/// The line the panel writes, at info, each time its main thread applies a
/// new edge and height to its window: once per applied change of either,
/// never per frame. It says where the panel now is, which is worth having
/// in a journal; and since only the main thread writes it, a test sees from
/// it that the main thread goes on working while the compositor does not
/// show the panel.
#[must_use]
pub fn placed(edge: Edge, height: Height) -> String {
    let edge = match edge {
        Edge::Bottom => "bottom",
        Edge::Top => "top",
    };
    format!(
        "the panel is placed along the {edge} edge, {} high",
        height.get()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_the_edge_and_the_height() {
        let height = Height::try_from(56).unwrap();
        assert_eq!(
            placed(Edge::Bottom, height),
            "the panel is placed along the bottom edge, 56 high"
        );
        assert_eq!(
            placed(Edge::Top, height),
            "the panel is placed along the top edge, 56 high"
        );
    }
}
