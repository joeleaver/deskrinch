//! Outputs that change while the wallpaper runs.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deskrinch_test_client::{Compositor, OutputChange, TestClient, configure_output};
use deskrinch_test_screen::Screen;
use deskrinch_test_wayland::Transform;

use crate::support::{
    BLUE, PATIENCE, Wallpaper, first_other, in_nested_session, wait_covered, wait_output,
    write_settings, written,
};

/// Turns the output `name` to `transform`, as the session's compositor
/// takes it: through `wlr-output-management` under sway and labwc, and
/// `kscreen-doctor` under KWin (as the session sets its outputs up).
fn turn(compositor: Compositor, name: &str, transform: Transform) {
    match compositor {
        Compositor::Sway | Compositor::Labwc => {
            let change = OutputChange {
                scale: None,
                transform: Some(transform),
            };
            configure_output(name, change).unwrap();
        }
        Compositor::Kwin => {
            let rotation = match transform {
                Transform::Normal => "none",
                Transform::Rotated90 => "left",
                Transform::Rotated180 => "inverted",
                Transform::Rotated270 => "right",
                Transform::Flipped => "flipped",
                Transform::Flipped90 => "flipped90",
                Transform::Flipped180 => "flipped180",
                Transform::Flipped270 => "flipped270",
            };
            let status = Command::new("kscreen-doctor")
                .env("QT_QPA_PLATFORM", "wayland")
                .arg(format!("output.{name}.rotation.{rotation}"))
                .status()
                .unwrap();
            assert!(status.success(), "kscreen-doctor: {status}");
        }
    }
}

/// Turns an output back as it was when dropped, so that a failed test
/// leaves the session's later tests the outputs they expect.
struct Turned {
    compositor: Compositor,
    name: String,
    was: Transform,
}

impl Drop for Turned {
    fn drop(&mut self) {
        turn(self.compositor, &self.name, self.was);
    }
}

/// The physical size of the output `name`, as captured.
fn captured_size(name: &str) -> (u32, u32) {
    let image = Screen::connect().unwrap().capture(name).unwrap();
    (image.width(), image.height())
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_output_turned_while_it_runs_is_covered_at_its_new_shape() {
    let compositor = in_nested_session();
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let _wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let name = Screen::connect().unwrap().outputs().remove(0);
    let client = TestClient::connect().unwrap();
    let was = client.output_info(&name).unwrap().transform;
    let (width, height) = captured_size(&name);
    // A quarter turn from what it is: its picture's sides swap.
    let to = if was == Transform::Normal {
        Transform::Rotated90
    } else {
        Transform::Normal
    };
    turn(compositor, &name, to);
    let _back = Turned {
        compositor,
        name: name.clone(),
        was,
    };
    let deadline = Instant::now() + PATIENCE;
    while captured_size(&name) != (height, width) {
        assert!(Instant::now() < deadline, "{name} did not turn");
        std::thread::sleep(Duration::from_millis(200));
    }
    wait_covered(BLUE);
}

/// The output names sway lists now.
fn sway_outputs() -> Vec<String> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_outputs", "-r"])
        .output()
        .unwrap();
    assert!(output.status.success(), "swaymsg: {output:?}");
    let listed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    listed
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|output| output["name"].as_str().map(ToOwned::to_owned))
        .collect()
}

/// Runs `swaymsg` with `args`.
fn swaymsg(args: &[&str]) {
    let status = Command::new("swaymsg")
        .args(args)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "swaymsg {args:?}: {status}");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn under_sway_an_output_added_gets_it_and_one_removed_loses_it() {
    if in_nested_session() != Compositor::Sway {
        // Only sway adds and removes outputs of a running test session.
        return;
    }
    write_settings(&format!("[wallpaper]\ncolor = \"{}\"\n", written(BLUE)));
    let mut wallpaper = Wallpaper::start();
    wait_covered(BLUE);
    let before = sway_outputs();
    swaymsg(&["create_output"]);
    let added = sway_outputs()
        .into_iter()
        .find(|name| !before.contains(name))
        .expect("an output added");
    wait_output(&added, |image| {
        first_other(image, BLUE).map(|at| format!("the new output is not all wallpaper: {at:?}"))
    });
    swaymsg(&["output", &added, "unplug"]);
    let deadline = Instant::now() + PATIENCE;
    while sway_outputs().contains(&added) {
        assert!(Instant::now() < deadline, "{added} was not removed");
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(wallpaper.running(), "the wallpaper ended with an output");
    wait_covered(BLUE);
}
