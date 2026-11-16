//! The pass with outputs of its own (`cargo xtask compositor-tests`' row
//! for this module): a 3840x2160 output beside a 1920x1080 one, as the
//! test client sees them.

use deskrinch_test_client::{outputs, require_outputs};

/// The session of this module's row has the outputs its pass states, one
/// beside the other at scale 1, upright.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_outputs_are_3840x2160_beside_1920x1080() {
    require_outputs(&[(3840, 2160), (1920, 1080)]);
    let mut layout = outputs().unwrap();
    // By name, the order the session made them: sway lists its heads in
    // another.
    layout.sort_by(|a, b| a.name.cmp(&b.name));
    let sizes: Vec<(u32, u32, (i32, i32), f64)> = layout
        .iter()
        .map(|output| {
            let mode = output.mode.unwrap();
            (mode.width, mode.height, output.position, output.scale)
        })
        .collect();
    assert_eq!(
        sizes,
        [(3840, 2160, (0, 0), 1.0), (1920, 1080, (3840, 0), 1.0)],
        "{layout:?}"
    );
}
