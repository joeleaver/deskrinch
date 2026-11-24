//! Each bar's taskbar shows the windows on its own output: a window is on
//! the bar of the output it says it is on (`TestWindow::wait_output`), and
//! not on the other's; opening it, or changing its title, draws only the
//! bar that shows it.

use deskrinch_taskbar::GRACE;
use deskrinch_test_client::{TestClient, TestWindow};
use deskrinch_test_component::{Mark, PerfWindow};
use deskrinch_test_input::Input;
use deskrinch_test_screen::{Screen, still_gap};
use deskrinch_test_wait::{Deadline, wait_for};
use tempfile::TempDir;

use crate::open_on::open_on;
use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};
use crate::taskbar::{RED, icon_in, install};

/// The frames the bar `window` of `panel` presented since `mark`.
fn presented_since(panel: &Panel, mark: &Mark, window: PerfWindow) -> u64 {
    panel
        .watch()
        .since(mark)
        .iter()
        .find(|since| since.window == window)
        .map_or(0, |since| since.frames)
}

/// A panel with the test's applications on two outputs, and one of red's
/// windows: the panel, the window and the client it is kept by, the output
/// the window is on and the other one, and what the panel's applications
/// are kept in.
struct OneWindow {
    panel: Panel,
    screen: Screen,
    window: TestWindow,
    _client: TestClient,
    on: String,
    other: String,
    band: Band,
    _dirs: TempDir,
}

impl OneWindow {
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
        let client = TestClient::connect().unwrap();
        let window = client.open("Red", RED.id).unwrap();
        let on = window.wait_output().unwrap();
        let other = screen
            .outputs()
            .into_iter()
            .find(|name| *name != on)
            .expect("a second output");
        let mut this = Self {
            panel,
            screen,
            window,
            _client: client,
            on,
            other,
            band,
            _dirs: dirs,
        };
        let on = this.on.clone();
        this.wait_shown_on(&on);
        this
    }

    /// Whether red's button is on the bar of `output`.
    fn shows(&mut self, output: &str) -> bool {
        let image = self.screen.capture(output).unwrap();
        icon_in(&image, self.band, scale_of(output), RED).is_some()
    }

    /// Waits until red's button is on `output`'s bar and on no other.
    fn wait_shown_on(&mut self, output: &str) {
        let outputs = self.screen.outputs();
        wait_for(&format!("red's button on {output}'s bar alone"), || {
            let shown: Vec<String> = outputs
                .iter()
                .filter(|name| self.shows(name))
                .cloned()
                .collect();
            if shown == [output.to_owned()] {
                Ok(())
            } else {
                Err(shown)
            }
        });
    }

    /// The frames `window` presented since `mark`.
    fn frames_since(&self, mark: &Mark, window: PerfWindow) -> u64 {
        self.panel
            .watch()
            .since(mark)
            .iter()
            .find(|since| since.window == window)
            .map_or(0, |since| since.frames)
    }
}

/// A window is on its own output's bar only, and its title changed draws
/// that bar alone: repeated until a change of title is the only thing that
/// happened in its round (the clock's minute draws every bar), the other
/// bar presenting nothing within the round's stability window.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_retitle_draws_only_the_bar_that_shows_the_window() {
    let this = OneWindow::start();
    let shown = this.panel.window_on(&this.on);
    let other = this.panel.window_on(&this.other);
    let deadline = Deadline::patience();
    let mut rounds = Vec::new();
    for round in 1_u32.. {
        let mark = this.panel.watch().mark();
        this.window
            .set_title(&format!("Red, round {round}"))
            .unwrap();
        wait_for(
            "the bar that shows the window to draw its title",
            || match this.frames_since(&mark, shown) {
                0 => Err("no frame yet"),
                _ => Ok(()),
            },
        );
        // The stability window: a frame of the other bar for the same
        // change would come within it.
        std::thread::sleep(still_gap());
        let drawn = this.frames_since(&mark, other);
        if drawn == 0 {
            return;
        }
        rounds.push((round, drawn));
        assert!(
            !deadline.passed(),
            "the other bar drew in every round, (round, frames): {rounds:?}; {}",
            this.panel.log()
        );
    }
}

/// A window moved to the other output leaves its bar and joins the other's,
/// each bar drawing one frame for each move. The move is the window made
/// fullscreen on the other output and then not; where it ends is the
/// compositor's (seen on KWin 6.6.6, sway 1.11 and labwc 0.9.3):
///
/// - KWin moves it there, and back when it leaves fullscreen: two moves.
/// - labwc moves it there and leaves it there; while fullscreen it covers
///   the bar it joined.
/// - sway moves it there and leaves it there, but says so to the panel only
///   once it leaves fullscreen.
///
/// So the test asks the window where it is once all that has settled, and
/// wants red's button on that output's bar alone, and the two bars to have
/// drawn as many frames as each other, one or two: a frame each per move.
/// A round that the clock's minute also drew is tried again.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_window_moved_leaves_one_bar_and_joins_the_other() {
    let mut this = OneWindow::start();
    let other = this.other.clone();
    let bars = [this.on.clone(), this.other.clone()].map(|name| this.panel.window_on(&name));
    let deadline = Deadline::patience();
    let mut rounds = Vec::new();
    loop {
        let mark = this.panel.watch().mark();
        this.window.set_fullscreen(true, Some(&other)).unwrap();
        this.window.wait_fullscreen().unwrap();
        this.window.set_fullscreen(false, None).unwrap();
        // Settled: red's button on the bar of the output the window is on,
        // alone, and no bar drawing again within the stability window.
        let mut last = None;
        let (on, frames) = wait_for("the move to settle", || {
            let on = this.window.outputs();
            let frames = bars.map(|bar| this.frames_since(&mark, bar));
            let [name] = on.as_slice() else {
                return Err(format!("the window is on {on:?}"));
            };
            let name = name.clone();
            let shown: Vec<String> = [this.on.clone(), this.other.clone()]
                .into_iter()
                .filter(|output| this.shows(output))
                .collect();
            if shown != [name.clone()] {
                last = None;
                return Err(format!(
                    "red's button is on {shown:?}, the window on {name}"
                ));
            }
            if last == Some(frames) {
                return Ok((name, frames));
            }
            last = Some(frames);
            std::thread::sleep(still_gap());
            Err(format!("the bars drew {frames:?}; keeping still?"))
        });
        let [a, b] = frames;
        if a == b && (1..=2).contains(&a) {
            return;
        }
        rounds.push((on, frames));
        assert!(
            !deadline.passed(),
            "the bars did not draw one frame each per move, (ended on, frames): {rounds:?}; {}",
            this.panel.log()
        );
    }
}

/// An ordinary window opened on one output draws nothing on the other
/// output's bar over its whole opening, its grace included: under sway and
/// KWin it is announced before it is placed, and a window shown on every
/// bar until then would draw there. Counted by that bar's frame lines, for
/// a window on each output in turn; a round the clock's minute drew is
/// tried again.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_ordinary_window_draws_nothing_on_another_outputs_bar() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut input = Input::connect().unwrap();
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::still(&mut screen);
    let panel = Panel::start();
    let band = Band::of(Edge::Bottom, 40, scale_of(&screen.outputs()[0]));
    wait_band(&mut screen, &before, band);
    let client = TestClient::connect().unwrap();
    let outputs = screen.outputs();
    for on in &outputs {
        let other = outputs
            .iter()
            .find(|name| *name != on)
            .expect("a second output");
        let (shown, elsewhere) = (panel.window_on(on), panel.window_on(other));
        let deadline = Deadline::patience();
        let mut rounds = Vec::new();
        loop {
            let mark = panel.watch().mark();
            let window = open_on(&mut input, &client, on, "Opened", "org.deskrinch.Opened");
            wait_for(
                &format!("{on}'s bar to draw the window"),
                || match presented_since(&panel, &mark, shown) {
                    0 => Err("no frame yet"),
                    _ => Ok(()),
                },
            );
            // The stability window, past the grace: a window shown on every
            // bar until it was placed, or once its grace was over, would
            // have drawn on the other bar within it.
            std::thread::sleep(GRACE + still_gap());
            let drawn = presented_since(&panel, &mark, elsewhere);
            drop(window);
            if drawn == 0 {
                break;
            }
            rounds.push(drawn);
            assert!(
                !deadline.passed(),
                "{other}'s bar drew for a window opened on {on} in every round: {rounds:?}; {}",
                panel.log()
            );
        }
    }
}
