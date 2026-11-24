//! A full taskbar on a 3840x2160 output, beside a 1920x1080 one (the pass
//! of `cargo xtask compositor-tests` with those outputs, which runs this
//! module alone), with more windows on each output than its bar has room
//! for: every bar's taskbar drawn full against the bar as it was before any
//! window, its buttons to the end with none missing and the count of those
//! left out after the last ([`strip`]); and, with the GPU
//! renderer, each frame's Vello usage under half of every buffer's first
//! size. A frame that ran out of buffer space would go out undrawn on a bar
//! in a window of its own (rinch issue 2781).

mod strip;

use std::fmt::Write as _;

use deskrinch_test_client::{TestClient, require_outputs};
use deskrinch_test_component::Renderer;
use deskrinch_test_input::Input;
use deskrinch_test_screen::Screen;
use deskrinch_test_wait::wait_for;
use deskrinch_window_management::{Action, WindowManagement, WindowState};

use crate::open_on::open_on;
use crate::support::{Band, Edge, Panel, Pictures, in_nested_session, panel_table, write_settings};
use crate::taskbar::window_of;

/// How many windows an output `width` logical pixels wide is given: more
/// than its bar has room for, so that the bar is full and says how many
/// it leaves out. A button with its icon alone and the gap after it are 32
/// logical pixels; a 27th of the width is more buttons than that (142 on
/// the 3840 output, 71 on the 1920 one), and no more, since each window
/// opened costs the compositor a tile.
fn windows_for(width: i32) -> usize {
    usize::try_from(width / 27).unwrap_or_default()
}

/// The counters of one of Vello's usage lines, `used … of …`, as
/// (name, used, size) for each buffer it names in both (the allocators'
/// `tile` is the sizes' `tiles`).
fn usage(line: &str) -> Vec<(String, u64, u64)> {
    let fields = |text: &str| -> Vec<(String, u64)> {
        text.split(['{', '}', ','])
            .filter_map(|part| {
                let (name, value) = part.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().parse().ok()?))
            })
            .collect()
    };
    let Some((used, sizes)) = line
        .split_once("BumpAllocators")
        .and_then(|(_, rest)| rest.split_once("BumpSizes"))
    else {
        return Vec::new();
    };
    let sizes = fields(sizes);
    fields(used)
        .into_iter()
        .filter_map(|(name, used)| {
            let sized_as = if name == "tile" {
                "tiles"
            } else {
                name.as_str()
            };
            let size = sizes.iter().find(|(sized, _)| sized == sized_as)?.1;
            Some((name, used, size))
        })
        .collect()
}

/// Makes the window of `app_id` the active one through the compositor's
/// window management, and waits until the compositor says it is.
fn activate(app_id: &str) {
    let (manager, id) = window_of(app_id);
    manager.perform(id, Action::Activate).unwrap();
    wait_for(&format!("{app_id} to be the active window"), || {
        let windows = manager.windows();
        let active = windows
            .iter()
            .any(|window| window.id == id && window.state.contains(WindowState::ACTIVE));
        if active { Ok(()) } else { Err(windows) }
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_full_taskbar_on_a_wide_output_is_drawn_whole() {
    in_nested_session();
    require_outputs(&[(3840, 2160), (1920, 1080)]);
    write_settings(&panel_table(Edge::Bottom, 40));
    // Before any window: on a headless seat, the seat's only pointer.
    let mut input = Input::connect().unwrap();
    let mut command = Panel::command();
    command.env("RUST_LOG", "warn,deskrinch_panel=info,vello=debug");
    let panel = Panel::start_with(command);
    let mut screen = Screen::connect().unwrap();
    let empty = Pictures::still(&mut screen);
    let client = TestClient::connect().unwrap();
    let outputs = screen.outputs();
    let named = |i: usize| (format!("Window {i}"), format!("org.deskrinch.wide.App{i}"));
    let mut windows = Vec::new();
    for output in &outputs {
        // The pointer onto the output for the first window only: the
        // compositors open the rest beside it. (Moved before each window,
        // sway 1.11 sent no `wl_surface.enter` within 120 s to the 65th
        // window opened on the output; the first 64 had theirs.)
        let (title, app_id) = named(0);
        windows.push(open_on(&mut input, &client, output, &title, &app_id));
        let (width, _) = client
            .output_info(output)
            .and_then(|info| info.logical_size)
            .unwrap_or_else(|| panic!("{output}'s logical size"));
        for i in 1..windows_for(width) {
            let (title, app_id) = named(i);
            windows.push(client.open(&title, &app_id).unwrap());
        }
    }
    let band = Band::of(Edge::Bottom, 40, 1);
    let read = |now: &Pictures| -> Result<Vec<strip::Full>, String> {
        let mut taskbars = Vec::new();
        let mut faults = Vec::new();
        for ((name, was), (_, is)) in empty.0.iter().zip(&now.0) {
            match strip::full(was, is, &band.rows_in(is.height())) {
                Ok(taskbar) => taskbars.push(taskbar),
                Err(why) => faults.push(format!("{name}: {why}")),
            }
        }
        if faults.is_empty() {
            Ok(taskbars)
        } else {
            Err(faults.join("; "))
        }
    };
    wait_for("every bar's taskbar to be drawn full", || {
        read(&Pictures::take(&mut screen, None).unwrap()).map(drop)
    });
    let full = Pictures::still(&mut screen);
    let taskbars = read(&full).unwrap_or_else(|why| panic!("{why}"));
    // One more window left out on each output: each bar's count changes,
    // and nothing among its buttons. The buttons are alike but for the
    // active one's mark, which stays on the last button of its output's bar
    // only while the newest window is the active one; a compositor need not
    // give the new window the focus (sway, on a pass, did not), so it is
    // made active, and the bars judged once it is.
    let mut newest = String::new();
    for output in &outputs {
        let (title, app_id) = named(windows.len());
        windows.push(open_on(&mut input, &client, output, &title, &app_id));
        newest = app_id;
    }
    activate(&newest);
    // That nothing among the buttons changes is what stays as it is: each
    // bar is judged on a still picture of its output.
    for ((name, was), &taskbar) in full.0.iter().zip(&taskbars) {
        Pictures::judged(
            &mut screen,
            name,
            &format!("{name}'s count to change for one more window, and nothing else"),
            |is| strip::count_changed(taskbar, was, is, &band.rows_in(is.height())),
        );
    }
    if Renderer::of_run() == Renderer::Gpu {
        let lines = panel.watch().lines();
        let mut report = String::new();
        let mut heaviest: Option<(f64, &str)> = None;
        for line in lines.iter().filter(|line| line.contains("vello: a ")) {
            assert!(!line.contains("ran out of buffer space"), "{line}");
            for (name, used, size) in usage(line) {
                assert!(
                    used * 2 < size,
                    "a frame used {used} of {name}'s {size}, half or more: {line}"
                );
                #[expect(clippy::cast_precision_loss, reason = "a share")]
                let share = used as f64 / size as f64;
                if heaviest.is_none_or(|(most, _)| share > most) {
                    heaviest = Some((share, line.as_str()));
                }
            }
        }
        if let Some((share, line)) = heaviest {
            writeln!(
                report,
                "the heaviest frame used {share:.3} of a buffer: {line}"
            )
            .unwrap();
        }
        println!("{report}");
    }
    drop(windows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_vellos_usage_line() {
        let line = "vello: a 3840x40 frame of 1024 path tags, 130 paths, 130 draw objects and \
                    16 clips used BumpAllocators { failed: 0, binning: 145, ptcl: 0, tile: 671 } \
                    of BumpSizes { lines: 16384, binning: 1024, tiles: 15360, ptcl: 122880 }";
        assert_eq!(
            usage(line),
            [
                ("binning".to_owned(), 145, 1024),
                ("ptcl".to_owned(), 0, 122_880),
                ("tile".to_owned(), 671, 15_360),
            ]
        );
    }
}
