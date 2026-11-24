//! A bar on every output, from the one panel process: each window keyed by
//! its output, an output that goes taking its bar with it and one that
//! comes back getting exactly one new bar, an output moved, rescaled or
//! turned keeping its bar, an edit of the settings moving every bar on its
//! own output, and every bar following what the panel shows (a window
//! opened, a title changed). Which windows the panel has is read from
//! rinch's per-window lines (`[PERF] window N opened/closed/frame`).

use std::collections::BTreeSet;
use std::io::Write as _;
use std::time::Instant;

use deskrinch_test_client::{
    OutputChange, OutputState, TestClient, change_output, outputs, turn_output_off,
};
use deskrinch_test_component::perf::parse;
use deskrinch_test_component::{Mark, PerfLine, PerfWindow, counter};
use deskrinch_test_screen::{Rgba, Screen, Transform};
use deskrinch_test_wait::wait_for;

use crate::frames::{drawn_between, drawn_since, step};
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

/// The panel at the bottom, 40 high, once each output's bar has presented
/// its first frame ([`Panel::start`]): for the tests that read the panel's
/// lines, not the screen.
fn start_panel() -> Panel {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    Panel::start()
}

/// The panel at the bottom, 40 high, once every output shows its bar on the
/// screen; the screen, for the tests that look again; and the outputs as
/// they were before the panel, to compare its bars with.
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
    let panel = start_panel();
    // No bar made or closed after the start: bounded by the bars' `placed`
    // lines for an edit made now.
    panel.bound(Edge::Top, 40);
    let names: BTreeSet<String> = TestClient::connect()
        .unwrap()
        .output_names()
        .into_iter()
        .collect();
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
    let panel = start_panel();
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
    // The kept bar kept, and no window made, for the output going: bounded
    // by the bars' `placed` lines for an edit made now.
    panel.bound(Edge::Top, 40);
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
    // Exactly one new bar, the kept one kept, for the output coming back:
    // bounded likewise, the new bar's `placed` line among them.
    panel.bound(Edge::Bottom, 40);
    let made: BTreeSet<PerfWindow> = ever(&panel).difference(&started).copied().collect();
    assert_eq!(
        made,
        [back].into(),
        "not exactly one new bar: {}",
        panel.log()
    );
    assert_eq!(panel.window_on(&kept_name), kept, "{}", panel.log());
}

/// How a test knows the panel has dealt with an output's change.
#[derive(Debug, Clone, Copy)]
enum Settled {
    /// The changed output's bar has presented at a size it was not before
    /// (a rescale, a turn), and put back, at the size it was.
    BySize,
    /// The bar keeps its size (a move): a window opened since has been
    /// drawn on every bar.
    ByAWindow,
}

/// The `surface_px` of the last frame `window` presented, among `lines`.
fn last_surface(lines: &[String], window: PerfWindow) -> Option<u64> {
    lines
        .iter()
        .rev()
        .filter(|line| matches!(parse(line), Some(PerfLine::Frame { window: drew, .. }) if drew == window))
        .find_map(|line| counter(line, "surface_px"))
}

/// Waits for what `settled` says after an output's change, its bar
/// `window` having presented at `surface` before it; `back` for the change
/// put back.
fn wait_settled(panel: &Panel, settled: Settled, window: PerfWindow, surface: u64, back: bool) {
    match settled {
        Settled::BySize => {
            wait_for(
                "the changed output's bar to present at its new size",
                || match last_surface(&panel.watch().lines(), window) {
                    Some(now) if (now == surface) == back => Ok(()),
                    now => Err(format!("{now:?}, {surface} before")),
                },
            );
        }
        Settled::ByAWindow => {
            let mark = panel.watch().mark();
            let client = TestClient::connect().unwrap();
            let opened = client.open("A window", "org.deskrinch.Outputs").unwrap();
            wait_each_bar_drew_since(panel, &mark, "for a window opened");
            drop(opened);
        }
    }
}

/// What is wrong with `drawn`, the frames of a bar nothing changed while
/// another output changed and settled as `settled` says, if anything: for
/// [`Settled::BySize`], a frame that repainted (`paint_frames`). Such a bar
/// may still get a frame: under sway and labwc, one served from its cached
/// pixels (its present skipped with the software renderer). Its line
/// carries only its own document's reactive work, none of the changed
/// bar's (rinch #2798: a window's frame line counts the effects, signal
/// writes and `ReRender`s of its own document). A [`Settled::ByAWindow`]
/// change repaints every bar for the window opened.
fn unchanged_repainted(settled: Settled, drawn: &[String]) -> Option<String> {
    match settled {
        Settled::ByAWindow => None,
        Settled::BySize => drawn
            .iter()
            .find(|frame| counter(frame, "paint_frames").is_some())
            .map(|frame| format!("repainted: {frame}")),
    }
}

/// Writes `line` to the standard error as it is, past the test harness's
/// capture of what a test prints.
fn say(line: &str) {
    // A line the log may lack is no reason to fail the test.
    writeln!(std::io::stderr(), "{line}").ok();
}

/// The second output changed by `change` (given the first output's state
/// and its own) and put back, each settled as `settled` says: the bars are
/// the ones the panel started with each time, and no window was made or
/// closed. For a change settled [`Settled::BySize`], the first output's
/// bar, which nothing changed, repaints nothing ([`unchanged_repainted`]).
fn keeps_its_bars(
    what: &str,
    settled: Settled,
    change: impl Fn(&OutputState, &OutputState) -> OutputChange,
) {
    // A span the clock drew inside is void (`spans::void`): nothing is
    // judged from it, and it is run again, once, on a panel started anew.
    // Not on the same panel: the span's bound moves the bars, and its
    // `placed` lines come before the move is drawn, so a span begun on the
    // same panel would read the first's move as a repaint (seen on sway,
    // twice in five runs). Waiting for that move's frame instead would rest
    // on every compositor sending a configure for a move at the same size,
    // which not every one is known to do. A second span the clock drew
    // inside fails: a span would then last as long as a tick.
    for attempt in 1..=2 {
        let panel = start_panel();
        let started = ever(&panel);
        let layout = outputs().unwrap();
        let (name, kept) = (layout[1].name.clone(), layout[0].name.clone());
        let (window, unchanged) = (panel.window_on(&name), panel.window_on(&kept));
        let surface = last_surface(&panel.watch().lines(), window).expect("the bar's frame");
        // Begun once the bar has drawn the clock's last text, so that a
        // text applied at the panel's start is not a frame inside the span.
        wait_for("the unchanged bar to draw the clock's last text", || {
            if crate::spans::pending(&panel.watch().lines(), unchanged) {
                Err(panel.log())
            } else {
                Ok(())
            }
        });
        let began = Instant::now();
        let from = step(&panel);
        let changed = change_output(&name, change(&layout[0], &layout[1])).unwrap();
        wait_settled(&panel, settled, window, surface, false);
        assert_eq!(open(&panel), started, "{what}: {}", panel.log());
        let (changing, middle) = drawn_since(&panel, &unchanged, from);
        changed.restore().unwrap();
        wait_settled(&panel, settled, window, surface, true);
        // The windows read from the change on are contiguous, so a late
        // repaint for it lands in the window for its putting back; that
        // last window ends at the bars' `placed` lines for an edit made
        // now, which bound everything before them. The edit's own move
        // comes after those lines, and is not counted.
        let to = panel.bound(Edge::Top, 40);
        let putting_back = drawn_between(&panel, &unchanged, middle, to);
        // Said straight to the standard error, which the test harness does
        // not capture, so that a run's log shows it whether the test passes
        // or fails.
        say(&format!(
            "{what}: span {attempt} took {:?}",
            began.elapsed()
        ));
        if crate::spans::void(&panel.watch().lines(), unchanged, from, to) {
            assert!(
                attempt == 1,
                "{what}: the clock drew inside both spans, on two panels: a span lasted as long \
                 as a tick"
            );
            say(&format!(
                "{what}: the clock drew inside span 1; it is void, and run again on a new panel"
            ));
            continue;
        }
        if let Some(why) = unchanged_repainted(settled, &changing) {
            panic!("{what}: the bar on {kept}, which nothing changed, {why}");
        }
        if let Some(why) = unchanged_repainted(settled, &putting_back) {
            panic!("{what}, put back: the bar on {kept}, which nothing changed, {why}");
        }
        assert_eq!(open(&panel), started, "{what}, put back: {}", panel.log());
        assert_eq!(
            ever(&panel),
            started,
            "{what}: a window was made or closed: {}",
            panel.log()
        );
        return;
    }
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_moved_keeps_its_bar() {
    keeps_its_bars("the move", Settled::ByAWindow, |first, _| {
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
    keeps_its_bars("the scale", Settled::BySize, |_, _| OutputChange {
        scale: Some(1.5),
        ..OutputChange::default()
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn an_output_turned_keeps_its_bar() {
    // A quarter turn from however the pass has it.
    keeps_its_bars("the turn", Settled::BySize, |_, second| OutputChange {
        transform: Some(if second.transform == Transform::Rotated90 {
            Transform::Normal
        } else {
            Transform::Rotated90
        }),
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
    // No window made or closed for the edit: bounded by the bars' `placed`
    // lines for a second edit made now.
    panel.bound(Edge::Bottom, 56);
    assert_eq!(open(&panel), started, "{}", panel.log());
    assert_eq!(ever(&panel), started, "a window was made: {}", panel.log());
}

/// Every bar shows the windows (until the taskbar is per output): a window
/// opened makes every bar draw, and so does its title changed.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn every_bar_follows_a_window_opened_and_retitled() {
    let panel = start_panel();
    let client = TestClient::connect().unwrap();
    let mark = panel.watch().mark();
    let window = client.open("A window", "org.deskrinch.Outputs").unwrap();
    wait_each_bar_drew_since(&panel, &mark, "for the window opened");
    let mark = panel.watch().mark();
    window.set_title("Retitled").unwrap();
    wait_each_bar_drew_since(&panel, &mark, "for the title changed");
}

/// The clock's one worker paints every bar's face once a tick: with
/// seconds shown, each bar draws as many frames as there were ticks (the
/// system time's whole seconds that began) over the same span, within the
/// one tick that may fall between the span's ends and a bar's read.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_clock_paints_each_bar_once_a_tick() {
    let panel = start_panel();
    write_settings(&format!(
        "{}\n[panel.clock]\nseconds = true\n",
        panel_table(Edge::Bottom, 40)
    ));
    // The clock restarted at seconds: count from a tick after that.
    let settled = panel.watch().mark();
    wait_each_bar_drew_since(&panel, &settled, "for the clock's first second");
    // The ticks are the texts the clock applied, as the panel says them
    // (`deskrinch_panel::clock_shows`), counted from the same moment as the
    // frames.
    let mark = panel.watch().mark();
    let from = step(&panel);
    let (counts, ticks) = wait_for("five ticks on every bar", || {
        let since = panel.watch().since(&mark);
        let ticks = panel.watch().lines()[from..]
            .iter()
            .filter(|line| line.contains(deskrinch_panel::CLOCK_SHOWS))
            .count();
        let counts: Vec<u64> = open(&panel)
            .iter()
            .map(|window| {
                since
                    .iter()
                    .find(|s| s.window == *window)
                    .map_or(0, |s| s.frames)
            })
            .collect();
        if counts.iter().all(|&frames| frames >= 5) {
            Ok((counts, u64::try_from(ticks).expect("a count of lines")))
        } else {
            Err(counts)
        }
    });
    assert!(counts.len() >= 2, "fewer than two bars: {counts:?}");
    assert!(
        counts.iter().all(|&frames| frames.abs_diff(ticks) <= 1),
        "the bars drew {counts:?} frames over {ticks} ticks: {}",
        panel.log()
    );
}

/// The colour of the bar on `output`, in its padding at its left end, half
/// way down the band.
fn bar_colour(screen: &mut Screen, output: &str) -> Rgba {
    let image = screen.capture(output).unwrap();
    let rows = Band::of(Edge::Bottom, 40, scale_of(output)).rows_in(image.height());
    let y = rows.start + (rows.end - rows.start) / 2;
    image.pixel(1, y).unwrap()
}

/// A change of colour scheme while the panel runs reaches every bar, not
/// only the first: each bar's own colour changes, and all end the same.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_change_of_scheme_reaches_every_bar() {
    in_nested_session();
    let settings = |scheme: &str| {
        format!(
            "[general]\ncolor-scheme = \"{scheme}\"\n{}",
            panel_table(Edge::Bottom, 40)
        )
    };
    write_settings(&settings("dark"));
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::still(&mut screen);
    let panel = Panel::start();
    let scale = scale_of(&screen.outputs()[0]);
    wait_band(&mut screen, &before, Band::of(Edge::Bottom, 40, scale));
    let bars: Vec<String> = panel.bars().into_iter().map(|(_, output)| output).collect();
    let dark: Vec<Rgba> = bars
        .iter()
        .map(|output| bar_colour(&mut screen, output))
        .collect();
    write_settings(&settings("light"));
    let light = wait_for("every bar in the light scheme", || {
        let now: Vec<Rgba> = bars
            .iter()
            .map(|output| bar_colour(&mut screen, output))
            .collect();
        let changed = now.iter().zip(&dark).all(|(now, was)| now != was);
        let alike = now.windows(2).all(|pair| pair[0] == pair[1]);
        if changed && alike { Ok(now) } else { Err(now) }
    });
    assert!(bars.len() >= 2, "fewer than two bars: {bars:?}");
    assert_eq!(light.len(), bars.len(), "{bars:?}");
}
