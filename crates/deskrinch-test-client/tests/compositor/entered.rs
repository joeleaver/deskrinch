//! A window says which output it is on: opened, it is on one of the
//! session's outputs; made fullscreen on an output, it is on that output
//! alone, having left any other. A window for each output, since a
//! compositor need not move a window already fullscreen to another output
//! asked for (KWin and labwc keep it where it is).

use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_wait::wait_for;

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_window_is_on_the_output_it_is_fullscreen_on() {
    let _nested = Compositor::current();
    let client = TestClient::connect().unwrap();
    let names = client.output_names();
    assert_eq!(names.len(), 2, "{names:?}");
    for name in &names {
        let window = client.open("Entered", "org.deskrinch.Entered").unwrap();
        let first = window.wait_output().unwrap();
        assert!(names.contains(&first), "{first:?} is none of {names:?}");
        window.set_fullscreen(true, Some(name)).unwrap();
        window.wait_fullscreen().unwrap();
        wait_for(&format!("the window to be on {name} alone"), || {
            let on = window.outputs();
            if on == std::slice::from_ref(name) {
                Ok(())
            } else {
                Err(on)
            }
        });
    }
}
