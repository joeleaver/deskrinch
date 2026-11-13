//! Outputs added and removed while the wallpaper runs: under sway only,
//! the one compositor of ours that adds and removes outputs of a running
//! test session (`swaymsg create_output`, `output … unplug`). The suites
//! table runs this module under sway alone, and skips it elsewhere.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deskrinch_test_client::Compositor;

use crate::support::{
    BLUE, PATIENCE, Wallpaper, first_other, in_nested_session, wait_covered, wait_output,
    write_settings, written,
};

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
#[ignore = "needs sway: cargo xtask compositor-tests runs it there"]
fn an_output_added_gets_it_and_one_removed_loses_it() {
    assert_eq!(
        in_nested_session(),
        Compositor::Sway,
        "this test adds and removes outputs, which only sway does here"
    );
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
