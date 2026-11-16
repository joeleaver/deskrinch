//! A full taskbar on a 3840x2160 output, beside a 1920x1080 one (the pass
//! of `cargo xtask compositor-tests` with those outputs, which runs this
//! module alone): every bar drawn whole across its width, and, with the GPU
//! renderer, each frame's Vello usage under half of every buffer's first
//! size. A frame that ran out of buffer space would go out undrawn on a bar
//! in a window of its own (rinch issue 2781).

use std::fmt::Write as _;

use deskrinch_test_client::{TestClient, require_outputs};
use deskrinch_test_component::Renderer;
use deskrinch_test_screen::Screen;
use deskrinch_test_wait::wait_for;

use crate::support::{Band, Edge, Panel, Pictures, in_nested_session, panel_table, write_settings};

/// More windows than the wide bar has room for, so that it is full and
/// says how many it leaves out.
const WINDOWS: usize = 140;

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

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_full_taskbar_on_a_wide_output_is_drawn_whole() {
    in_nested_session();
    require_outputs(&[(3840, 2160), (1920, 1080)]);
    write_settings(&panel_table(Edge::Bottom, 40));
    let mut screen = Screen::connect().unwrap();
    let before = Pictures::still(&mut screen);
    let mut command = Panel::command();
    command.env("RUST_LOG", "warn,deskrinch_panel=info,vello=debug");
    let panel = Panel::start_with(command);
    let client = TestClient::connect().unwrap();
    let windows: Vec<_> = (0..WINDOWS)
        .map(|i| {
            client
                .open(
                    &format!("Window {i}"),
                    &format!("org.deskrinch.wide.App{i}"),
                )
                .unwrap()
        })
        .collect();
    // Every column of every bar differs from what was under it: drawn
    // whole, its last button and its count of those left out included.
    wait_for("every bar to be drawn whole across its width", || {
        let now = Pictures::still(&mut screen);
        let faults: Vec<String> = before
            .0
            .iter()
            .zip(&now.0)
            .filter_map(|((name, was), (_, is))| {
                let rows = Band::of(Edge::Bottom, 40, 1).rows_in(is.height());
                let blank = (0..is.width())
                    .find(|&x| rows.clone().all(|y| was.pixel(x, y) == is.pixel(x, y)))?;
                Some(format!("{name}'s bar is not drawn at column {blank}"))
            })
            .collect();
        if faults.is_empty() {
            Ok(())
        } else {
            Err(faults.join("; "))
        }
    });
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
