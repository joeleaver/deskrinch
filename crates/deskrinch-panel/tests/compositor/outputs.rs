//! A bar on every output, from the one panel process: each window keyed by
//! its output, an output that goes taking its bar with it and one that
//! comes back getting exactly one new bar, an output moved, rescaled or
//! turned keeping its bar, an edit of the settings moving every bar on its
//! own output, and every bar following what the panel shows (a window
//! opened, a title changed). Which windows the panel has is read from
//! rinch's per-window lines (`[PERF] window N opened/closed/frame`).

use std::collections::BTreeSet;

use deskrinch_test_client::{
    OutputChange, OutputState, TestClient, change_output, outputs, turn_output_off,
};
use deskrinch_test_component::{Mark, PerfWindow};
use deskrinch_test_screen::{Screen, Transform};
use deskrinch_test_wait::wait_for;

use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};

/// Every window of the panel's the watch still lists: those open, and
/// closed ones it has not dropped (a closed window may leave the list).
fn ever(panel: &Panel) -> BTreeSet<PerfWindow> {
    panel
        .watch()
        .windows()
        .into_iter()
        .map(|state| state.window)
        .collect()
}

/// The panel's bars open now, by window.
fn open(panel: &Panel) -> BTreeSet<PerfWindow> {
    panel.bars().into_iter().map(|(window, _)| window).collect()
}

/// Waits until each bar open now has presented a frame since `mark`.
fn wait_each_bar_drew_since(panel: &Panel, mark: &Mark, what: &str) {
    let watch = panel.watch();
    wait_for(&format!("every bar to draw {what}"), || {
        let since = watch.since(mark);
        let open = open(panel);
        let drew = open
            .iter()
            .all(|window| since.iter().any(|s| s.window == *window && s.frames > 0));
        if drew { Ok(()) } else { Err(since) }
    });
}

/// The panel at the bottom, 40 high, once every output shows its bar; the
/// screen, for the tests that look again; and the outputs as they were
/// before the panel, to compare its bars with.
fn start_bars() -> (Panel, Screen, Pictures) {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::still(&mut screen);
    let panel = Panel::start();
    let scale = scale_of(&screen.outputs()[0]);
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    (panel, screen, before)
}

/// Every output has a bar of its own: one open window per output, each
/// placed on a different output, and no other window made.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn every_output_has_its_own_bar() {
    let (panel, screen, _) = start_bars();
    let names: BTreeSet<String> = screen.outputs().into_iter().collect();
    let on: BTreeSet<String> = panel.bars().into_iter().map(|(_, output)| output).collect();
    assert_eq!(on, names, "the bars: {:?}", panel.bars());
    assert_eq!(panel.bars().len(), names.len(), "{:?}", panel.bars());
    assert_eq!(
        ever(&panel),
        open(&panel),
        "a window closed: {}",
        panel.log()
    );
}

/// An output turned off takes its bar with it, the other keeping its own;
/// turned on again, it gets exactly one new bar. To the panel this is an
/// output unplugged and one plugged in: its row goes with the output's id,
/// and a new id is a new row.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_turned_off_and_on_gets_exactly_one_new_bar() {
    let (panel, _screen, _) = start_bars();
    let layout = outputs().unwrap();
    let (gone_name, kept_name) = (layout[1].name.clone(), layout[0].name.clone());
    let gone = panel.window_on(&gone_name);
    let kept = panel.window_on(&kept_name);
    let started = ever(&panel);

    let off = turn_output_off(&gone_name).unwrap();
    wait_for(&format!("{gone_name}'s bar to close"), || {
        if open(&panel).contains(&gone) {
            Err(panel.log())
        } else {
            Ok(())
        }
    });
    assert_eq!(panel.window_on(&kept_name), kept, "{}", panel.log());
    assert!(
        ever(&panel).is_subset(&started),
        "a window was made: {}",
        panel.log()
    );

    off.restore().unwrap();
    let back = wait_for(&format!("a new bar on {gone_name} to draw"), || {
        panel
            .bars()
            .into_iter()
            .find(|(window, on)| *on == gone_name && !started.contains(window))
            .filter(|(window, _)| {
                panel
                    .watch()
                    .windows()
                    .iter()
                    .any(|state| state.window == *window && state.first_frame.is_some())
            })
            .map(|(window, _)| window)
            .ok_or_else(|| panel.log())
    });
    let made: BTreeSet<PerfWindow> = ever(&panel).difference(&started).copied().collect();
    assert_eq!(
        made,
        [back].into(),
        "not exactly one new bar: {}",
        panel.log()
    );
    assert_eq!(panel.window_on(&kept_name), kept, "{}", panel.log());
}

/// The second output changed by `change` (given the first output's state)
/// and put back: the bars are the ones the panel started with each time,
/// each having drawn since, and no other window was made.
fn keeps_its_bars(what: &str, change: impl FnOnce(&OutputState) -> OutputChange) {
    let (panel, _screen, _) = start_bars();
    let started = ever(&panel);
    let layout = outputs().unwrap();
    let name = layout[1].name.clone();
    let mark = panel.watch().mark();
    let changed = change_output(&name, change(&layout[0])).unwrap();
    wait_each_bar_drew_since(&panel, &mark, &format!("after {what}"));
    assert_eq!(open(&panel), started, "{what}: {}", panel.log());
    let mark = panel.watch().mark();
    changed.restore().unwrap();
    wait_each_bar_drew_since(&panel, &mark, &format!("after {what}, put back"));
    assert_eq!(open(&panel), started, "{what}, put back: {}", panel.log());
    assert_eq!(
        ever(&panel),
        started,
        "{what}: a window was made: {}",
        panel.log()
    );
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_moved_keeps_its_bar() {
    keeps_its_bars("the move", |first| {
        let (_, height) = TestClient::connect()
            .unwrap()
            .output_info(&first.name)
            .and_then(|info| info.logical_size)
            .expect("the first output's logical size");
        OutputChange {
            position: Some((first.position.0, first.position.1 + height)),
            ..OutputChange::default()
        }
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_rescaled_keeps_its_bar() {
    keeps_its_bars("the scale", |_| OutputChange {
        scale: Some(1.5),
        ..OutputChange::default()
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_turned_keeps_its_bar() {
    keeps_its_bars("the turn", |_| OutputChange {
        transform: Some(Transform::Rotated90),
        ..OutputChange::default()
    });
}

/// An edit of the edge and height moves every bar, each on its own output
/// (each bar moves its own window through its handle: a bar that moved
/// another's would leave one output with its bar where it was), and makes
/// no new window.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_edit_moves_every_bar_each_on_its_own_output() {
    let (panel, mut screen, before) = start_bars();
    let started = ever(&panel);
    let scale = scale_of(&screen.outputs()[0]);
    write_settings(&panel_table(Edge::Top, 56));
    wait_band(&mut screen, &before, Band::of(Edge::Top, 56, scale));
    assert_eq!(open(&panel), started, "{}", panel.log());
    assert_eq!(ever(&panel), started, "a window was made: {}", panel.log());
}

/// Every bar shows the windows (until the taskbar is per output): a window
/// opened makes every bar draw, and so does its title changed.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn every_bar_follows_a_window_opened_and_retitled() {
    let (panel, _screen, _) = start_bars();
    let client = TestClient::connect().unwrap();
    let mark = panel.watch().mark();
    let window = client.open("A window", "org.deskrinch.Outputs").unwrap();
    wait_each_bar_drew_since(&panel, &mark, "for the window opened");
    let mark = panel.watch().mark();
    window.set_title("Retitled").unwrap();
    wait_each_bar_drew_since(&panel, &mark, "for the title changed");
}

/// The clock's one worker paints every bar's face once a tick: with
/// seconds shown, over several ticks each bar draws as many frames as the
/// others, within the one tick that may fall between two bars' reads.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_clock_paints_each_bar_once_a_tick() {
    let (panel, _screen, _) = start_bars();
    write_settings(&format!(
        "{}\n[panel.clock]\nseconds = true\n",
        panel_table(Edge::Bottom, 40)
    ));
    // The clock restarted at seconds: count from a tick after that.
    let settled = panel.watch().mark();
    wait_each_bar_drew_since(&panel, &settled, "for the clock's first second");
    let mark = panel.watch().mark();
    let counts = wait_for("three ticks on every bar", || {
        let since = panel.watch().since(&mark);
        let counts: Vec<u64> = open(&panel)
            .iter()
            .map(|window| {
                since
                    .iter()
                    .find(|s| s.window == *window)
                    .map_or(0, |s| s.frames)
            })
            .collect();
        if counts.iter().all(|&frames| frames >= 3) {
            Ok(counts)
        } else {
            Err(counts)
        }
    });
    let (fewest, most) = (counts.iter().min(), counts.iter().max());
    assert!(
        most.zip(fewest)
            .is_some_and(|(most, fewest)| most - fewest <= 1),
        "the bars drew {counts:?} frames over the same ticks: {}",
        panel.log()
    );
}
