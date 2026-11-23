//! Layer surfaces with the namespace, layer and size a test chooses: each
//! is mapped and drawn at the size it asked for, beside another.

use deskrinch_test_client::{Compositor, ShellLayer, TestClient};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn layer_surfaces_are_made_as_asked() {
    Compositor::current();
    let client = TestClient::connect().unwrap();
    let first = client
        .open_layer("deskrinch-test-first", ShellLayer::Bottom, (90, 30))
        .unwrap();
    let second = client
        .open_layer("deskrinch-test-second", ShellLayer::Top, (70, 50))
        .unwrap();
    assert_eq!(first.drawn(), Some((90, 30)));
    assert_eq!(second.drawn(), Some((70, 50)));
    // Without the keyboard: neither asked for it.
    assert!(!first.has_keyboard());
    assert!(!second.has_keyboard());
}
