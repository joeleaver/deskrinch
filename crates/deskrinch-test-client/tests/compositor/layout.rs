//! Changes to the session's outputs put back exactly as they were.
//!
//! Before the guards, turning an output off and on left sway with the
//! output at (0,0), on top of the other, and KWin with the second output
//! at (0,0) once the first had been off. Each test checks every output's
//! position, mode, scale and transform against the layout it started with.

use std::panic::{self, AssertUnwindSafe};

use deskrinch_test_client::{
    Compositor, OutputChange, OutputState, change_output, layout_differences, outputs,
    turn_output_off,
};
use deskrinch_test_wayland::Transform;

/// The session's layout, after checking it has the two outputs these tests
/// need.
fn fresh() -> Vec<OutputState> {
    let _nested = Compositor::current();
    let layout = outputs().unwrap();
    assert_eq!(layout.len(), 2, "{layout:?}");
    assert!(layout.iter().all(|output| output.enabled), "{layout:?}");
    layout
}

/// Fails unless the session's layout is `before`.
fn assert_as(before: &[OutputState], after_what: &str) {
    if let Some(differences) = layout_differences(before, &outputs().unwrap()) {
        panic!("after {after_what}, {differences}");
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn each_output_turned_off_and_put_back_leaves_the_layout_as_it_was() {
    let before = fresh();
    for output in &before {
        let restore = turn_output_off(&output.name).unwrap();
        let off = outputs().unwrap();
        let this = off.iter().find(|o| o.name == output.name).unwrap();
        assert!(!this.enabled, "{off:?}");
        restore.restore().unwrap();
        assert_as(&before, &format!("{} off and back", output.name));
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn each_output_turned_and_put_back_leaves_the_layout_as_it_was() {
    let before = fresh();
    for output in &before {
        let quarter = OutputChange {
            scale: None,
            transform: Some(Transform::Rotated90),
        };
        let restore = change_output(&output.name, quarter).unwrap();
        let turned = outputs().unwrap();
        let this = turned.iter().find(|o| o.name == output.name).unwrap();
        assert_eq!(this.transform, Transform::Rotated90, "{turned:?}");
        restore.restore().unwrap();
        assert_as(&before, &format!("{} turned and back", output.name));
    }
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_guard_dropped_puts_the_layout_back() {
    let before = fresh();
    let first = &before[0].name;
    drop(turn_output_off(first).unwrap());
    assert_as(&before, "a guard dropped");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_guard_dropped_by_a_panic_puts_the_layout_back() {
    let before = fresh();
    let first = before[0].name.clone();
    let failed = panic::catch_unwind(AssertUnwindSafe(|| {
        let _restore = turn_output_off(&first).unwrap();
        panic!("a test failing with an output off");
    }));
    assert!(failed.is_err());
    assert_as(&before, "a guard dropped by a panic");
}
