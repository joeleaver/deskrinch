//! The windows sway 1.11 never places while they stay as they opened (it
//! sends a window's `output_enter` as the window is shown on an output):
//! one opened on a workspace not shown, and one opened fullscreen. With
//! two outputs, each is on every bar once its grace is over, where it
//! cannot be told; and the hidden one is on its own output's bar alone
//! once its workspace is shown. sway hides the bar under a fullscreen
//! window, so that bar is seen to draw by its frame lines, not by the
//! screen. Run by a row of its own, under sway.

use std::process::Command;

use deskrinch_test_client::{Paint, TestClient};
use deskrinch_test_screen::Screen;
use deskrinch_test_wait::wait_for;
use tempfile::TempDir;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};
use crate::taskbar::{App, FULLSCREEN, HIDDEN, HIDDEN_TOO, icon_in, install};

/// Runs `command` through `swaymsg`.
///
/// # Panics
///
/// When sway refuses it.
fn swaymsg(command: &str) {
    let out = Command::new("swaymsg").arg(command).output().unwrap();
    assert!(
        out.status.success(),
        "swaymsg {command}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The panel with the test's applications, along the bottom of two outputs.
struct Bars {
    _dirs: TempDir,
    panel: Panel,
    screen: Screen,
    band: Band,
    client: TestClient,
}

impl Bars {
    fn start() -> Self {
        in_nested_session();
        write_settings(&panel_table(Edge::Bottom, 40));
        let dirs = tempfile::tempdir().unwrap();
        install(dirs.path());
        let mut screen = Screen::connect().unwrap();
        let before = Pictures::still(&mut screen);
        let mut command = Panel::command();
        command.env("XDG_DATA_HOME", dirs.path());
        let panel = Panel::start_with(command);
        let band = Band::of(Edge::Bottom, 40, scale_of(&screen.outputs()[0]));
        wait_band(&mut screen, &before, band);
        Self {
            _dirs: dirs,
            panel,
            screen,
            band,
            client: TestClient::connect().unwrap(),
        }
    }

    /// The outputs whose bar shows `app`'s button, among `outputs`.
    fn showing(&mut self, app: App, outputs: &[String]) -> Vec<String> {
        outputs
            .iter()
            .filter(|output| {
                let image = self.screen.capture(output).unwrap();
                icon_in(&image, self.band, scale_of(output), app).is_some()
            })
            .cloned()
            .collect()
    }

    /// Waits until `app`'s button is on exactly the bars of `outputs`.
    fn wait_showing(&mut self, app: App, outputs: &[String], what: &str) {
        let all = self.screen.outputs();
        wait_for(what, || {
            let shown = self.showing(app, &all);
            if shown == outputs { Ok(()) } else { Err(shown) }
        });
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_opened_on_a_workspace_not_shown_is_on_every_bar_then_its_own() {
    let mut bars = Bars::start();
    let outputs = bars.screen.outputs();
    swaymsg(&format!(
        "for_window [app_id=\"{}\"] move container to workspace 9",
        HIDDEN.id
    ));
    let window = bars.client.open("Hidden", HIDDEN.id).unwrap();
    bars.wait_showing(
        HIDDEN,
        &outputs,
        "the unplaced window on every bar after its grace",
    );
    // Its workspace shown: sway places it, and it leaves the other bar.
    swaymsg("workspace 9");
    let on = window.wait_output().unwrap();
    bars.wait_showing(
        HIDDEN,
        std::slice::from_ref(&on),
        &format!("the window on {on}'s bar alone"),
    );
    swaymsg("workspace back_and_forth");
}

/// Two windows opened on a workspace not shown, one just after the other:
/// both graces end within a moment, and both windows are on every bar
/// after them, whichever end the one timer wakes for.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn two_windows_opened_on_a_workspace_not_shown_are_both_on_every_bar() {
    let mut bars = Bars::start();
    let outputs = bars.screen.outputs();
    for app in [HIDDEN, HIDDEN_TOO] {
        swaymsg(&format!(
            "for_window [app_id=\"^{}$\"] move container to workspace 9",
            app.id
        ));
    }
    let _first = bars.client.open("Hidden", HIDDEN.id).unwrap();
    let _second = bars.client.open("Hidden too", HIDDEN_TOO.id).unwrap();
    for app in [HIDDEN, HIDDEN_TOO] {
        bars.wait_showing(
            app,
            &outputs,
            &format!("{} on every bar after its grace", app.id),
        );
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_opened_fullscreen_is_on_every_bar_after_its_grace() {
    let mut bars = Bars::start();
    let outputs = bars.screen.outputs();
    let (under, other) = (outputs[1].clone(), outputs[0].clone());
    let covered = bars.panel.window_on(&under);
    let mark = bars.panel.watch().mark();
    let _window = bars
        .client
        .open_painted(
            "Fullscreen",
            FULLSCREEN.id,
            Paint::Transparent,
            Some(&under),
        )
        .unwrap();
    // sway hides the bar under a fullscreen window: the bar not under it
    // shows the button once the grace is over, and the one under it drew
    // for it, presented or prepared while hidden.
    bars.wait_showing(
        FULLSCREEN,
        &[other],
        "the unplaced fullscreen window on the bar not under it",
    );
    wait_for(
        &format!("{under}'s bar, under the window, to draw for it"),
        || {
            let since = bars.panel.watch().since(&mark);
            let drew = since
                .iter()
                .find(|since| since.window == covered)
                .map_or(0, |since| since.frames + since.prepared);
            if drew > 0 { Ok(()) } else { Err(since) }
        },
    );
}
