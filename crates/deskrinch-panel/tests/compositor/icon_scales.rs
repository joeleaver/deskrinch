//! Each bar draws its icons at its own output's scale, with two outputs at
//! different scales: the icon theme has the application's icon in two
//! files, green for scale 1 and magenta for scale 2, so the colour a bar
//! shows says which file it drew. Read after each bar's first frame, as the
//! scale a window reads may only come with its window (rinch issue 2789:
//! until rinch's fix, a bar in a window of its own reads the process's
//! scale, not its window's, and this test fails on the output at scale 2).

use std::fs;
use std::path::Path;

use deskrinch_test_client::{OutputChange, TestClient, change_output, outputs};
use deskrinch_test_screen::{Rgba, Screen};
use deskrinch_test_wait::wait_for;

use crate::support::{Band, Edge, Panel, in_nested_session, panel_table, scale_of, write_settings};

/// The application whose window the bars show.
const APP: &str = "org.deskrinch.taskbar.Scales";
/// Its icon at scale 1.
const AT_ONE: Rgba = Rgba::opaque(40, 200, 40);
/// Its icon at scale 2.
const AT_TWO: Rgba = Rgba::opaque(200, 40, 200);

/// The theme, with the icon in a directory for each scale.
const INDEX: &str = "[Icon Theme]\nName=Hicolor\nDirectories=16x16/apps,16x16@2/apps\n\n\
                     [16x16/apps]\nSize=16\nType=Fixed\n\n\
                     [16x16@2/apps]\nSize=16\nScale=2\nType=Fixed\n";

/// A 16x16 icon of one colour.
fn svg(colour: Rgba) -> String {
    let Rgba { r, g, b, .. } = colour;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" \
         viewBox=\"0 0 16 16\"><rect width=\"16\" height=\"16\" \
         fill=\"#{r:02x}{g:02x}{b:02x}\"/></svg>\n"
    )
}

/// The application and its icons under `data` (`XDG_DATA_HOME`).
fn install(data: &Path) {
    let theme = data.join("icons/hicolor");
    for (dir, colour) in [("16x16/apps", AT_ONE), ("16x16@2/apps", AT_TWO)] {
        fs::create_dir_all(theme.join(dir)).unwrap();
        fs::write(theme.join(dir).join(format!("{APP}.svg")), svg(colour)).unwrap();
    }
    fs::write(theme.join("index.theme"), INDEX).unwrap();
    let applications = data.join("applications");
    fs::create_dir_all(&applications).unwrap();
    fs::write(
        applications.join(format!("{APP}.desktop")),
        format!("[Desktop Entry]\nType=Application\nName=Scales\nExec=/bin/false\nIcon={APP}\n"),
    )
    .unwrap();
}

/// Whether the band of the output `name` shows `colour` anywhere.
fn shows(screen: &mut Screen, name: &str, colour: Rgba) -> bool {
    let image = screen.capture(name).unwrap();
    let band = Band::of(Edge::Bottom, 40, scale_of(name)).rows_in(image.height());
    band.into_iter()
        .filter_map(|y| image.row(y))
        .any(|row| row.contains(&colour))
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn each_bar_draws_its_icons_at_its_outputs_scale() {
    in_nested_session();
    write_settings(&panel_table(Edge::Bottom, 40));
    let layout = outputs().unwrap();
    let second = layout[1].name.clone();
    let other = if scale_of(&layout[0].name) == 1 {
        2.0
    } else {
        1.0
    };
    let changed = change_output(
        &second,
        OutputChange {
            scale: Some(other),
            ..OutputChange::default()
        },
    )
    .unwrap();
    let dirs = tempfile::tempdir().unwrap();
    install(dirs.path());
    let mut command = Panel::command();
    command.env("XDG_DATA_HOME", dirs.path());
    let panel = Panel::start_with(command);
    let client = TestClient::connect().unwrap();
    let _window = client.open("Scales", APP).unwrap();
    let mut screen = Screen::connect().unwrap();
    for (_, output) in panel.bars() {
        let (want, not) = if scale_of(&output) >= 2 {
            (AT_TWO, AT_ONE)
        } else {
            (AT_ONE, AT_TWO)
        };
        wait_for(
            &format!(
                "{output}'s bar to draw the icon of its scale {}",
                scale_of(&output)
            ),
            || {
                if shows(&mut screen, &output, want) && !shows(&mut screen, &output, not) {
                    Ok(())
                } else {
                    Err(format!(
                        "it shows the scale-1 icon: {}, the scale-2 icon: {}",
                        shows(&mut screen, &output, AT_ONE),
                        shows(&mut screen, &output, AT_TWO)
                    ))
                }
            },
        );
    }
    drop(panel);
    changed.restore().unwrap();
}
