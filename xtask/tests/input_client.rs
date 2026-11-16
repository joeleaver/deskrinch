//! `cargo xtask input` takes what its lines say to where they say it: a
//! `point` line puts the pointer at the point on the output it names, and a
//! `key` line types the key, both heard by a window of the test's own
//! (`deskrinch-test-client`). The measure tests' stand-ins hear nothing, and
//! `deskrinch-test-input`'s own tests do not go through the client's lines.
//!
//! Run by `cargo xtask compositor-tests` under sway and labwc. Under KWin
//! the client, xtask's own binary, would have to be granted fake input, and
//! the suites table grants only a suite's test binary and the shell's
//! components.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use deskrinch_test_client::{Compositor, Paint, TestClient};
use deskrinch_test_input::Key;
use deskrinch_test_wayland::Rgba;

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_point_and_a_key_reach_the_window_as_sent() {
    let _nested = Compositor::current();
    // The client first: on a headless wlroots seat its keyboard is the
    // seat's, and a window mapped before it would get no keyboard focus.
    let mut client = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("input")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut lines = client.stdin.take().unwrap();
    let mut said = BufReader::new(client.stdout.take().unwrap()).lines();
    let mut answer = || said.next().unwrap().unwrap();
    assert_eq!(answer(), "ready");

    let windows = TestClient::connect().unwrap();
    let output = windows.output_names().into_iter().next().unwrap();
    let paint = Paint::Solid(Rgba::opaque(40, 90, 160));
    let window = windows
        .open_painted(
            "pointed at",
            "org.deskrinch.InputClientTest",
            paint,
            Some(&output),
        )
        .unwrap();
    window.wait_fullscreen().unwrap();

    // A pixel beside the point first, so that the second is a motion the
    // window hears even if the cursor was already there.
    let mark = window.pointer_mark();
    for line in [
        format!("point {output} 101 80"),
        format!("point {output} 100 80"),
    ] {
        writeln!(lines, "{line}").unwrap();
        assert_eq!(answer(), "done", "{line}");
    }
    window
        .wait_pointer_at_since(mark, 100.0, 80.0)
        .unwrap_or_else(|error| panic!("{error}: {:?}", window.pointer_events()));

    window.wait_keyboard().unwrap();
    for line in ["key o", "key  "] {
        writeln!(lines, "{line}").unwrap();
        assert_eq!(answer(), "done", "{line:?}");
    }
    let space = Key::typing(' ').unwrap().code();
    window.wait_key(space).unwrap();
    assert_eq!(window.keys(), [Key::typing('o').unwrap().code(), space]);

    drop(lines);
    assert!(client.wait().unwrap().success());
}
