//! Each bar draws its icons at its own output's scale, with two outputs at
//! different scales: the icon theme has the application's icon in two
//! files, green for scale 1 and magenta for scale 2, so the colour a bar
//! shows says which file it drew.
//!
//! Both tests are regression tests, and pass today: a bar reads its
//! window's scale when it makes a button, and the taskbar makes its buttons
//! only after the bar's first layout has given it a width, by when the
//! window's own facts are there. Were a button made during the bar's
//! mount, it would read the process's scale and keep it (rinch issue 2789),
//! and the bar on the output at scale 2 would draw the scale-1 icon.

use std::fs;
use std::path::Path;

use deskrinch_test_client::{
    OutputChange, RestoreOutputs, TestClient, TestWindow, change_output, outputs, turn_output_off,
};
use deskrinch_test_input::Input;
use deskrinch_test_screen::{Image, Rgba, Screen};
use deskrinch_test_wait::wait_for;

use crate::open_on::open_on;
use crate::support::{
    Band, Edge, Panel, Pictures, in_nested_session, panel_table, scale_of, write_settings,
};

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

/// Whether `image`, the output `name`'s, shows `colour` anywhere in its band.
fn shows(image: &Image, name: &str, colour: Rgba) -> bool {
    let band = Band::of(Edge::Bottom, 40, scale_of(name)).rows_in(image.height());
    band.into_iter()
        .filter_map(|y| image.row(y))
        .any(|row| row.contains(&colour))
}

/// Waits until the bar on `output` draws the icon of `output`'s scale and
/// not the other's.
fn wait_icon_of_its_scale(screen: &mut Screen, output: &str) {
    let (want, not) = if scale_of(output) >= 2 {
        (AT_TWO, AT_ONE)
    } else {
        (AT_ONE, AT_TWO)
    };
    // That the other scale's icon is not drawn is what stays as it is:
    // both are read on one still picture ([`Pictures::judged`]).
    Pictures::judged(
        screen,
        output,
        &format!(
            "{output}'s bar to draw the icon of its scale {}",
            scale_of(output)
        ),
        |image| {
            if shows(image, output, want) && !shows(image, output, not) {
                Ok(())
            } else {
                Err(format!(
                    "it shows the scale-1 icon: {}, the scale-2 icon: {}",
                    shows(image, output, AT_ONE),
                    shows(image, output, AT_TWO)
                ))
            }
        },
    );
}

/// The outputs at different scales (the second at the scale the first has
/// not), the panel started with the theme of [`install`], and a window of
/// the application on each output, so that every bar has an icon, kept
/// with the pointer and the client that made them; the second output's
/// change, to put back.
struct TwoScales {
    panel: Panel,
    _input: Input,
    _client: TestClient,
    _windows: Vec<TestWindow>,
    changed: RestoreOutputs,
    _dirs: tempfile::TempDir,
}

impl TwoScales {
    fn start() -> Self {
        in_nested_session();
        write_settings(&panel_table(Edge::Bottom, 40));
        // Before any window: on a headless seat, the seat's only pointer.
        let mut input = Input::connect().unwrap();
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
        let windows = layout
            .iter()
            .map(|output| open_on(&mut input, &client, &output.name, "Scales", APP))
            .collect();
        Self {
            panel,
            _input: input,
            _client: client,
            _windows: windows,
            changed,
            _dirs: dirs,
        }
    }

    /// Ends the panel and puts the outputs back.
    fn end(self) {
        drop(self.panel);
        self.changed.restore().unwrap();
    }
}

/// A bar made after the panel started, on an output at scale 2 turned off
/// and on again, draws its icons at scale 2, the icon index already in
/// place when the bar mounts: a window opened on that output once its bar
/// is there (sway leaves the first one on the other output).
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_bar_made_later_draws_its_icons_at_its_outputs_scale() {
    let two = TwoScales::start();
    let panel = &two.panel;
    let mut screen = Screen::connect().unwrap();
    for (_, output) in panel.bars() {
        wait_icon_of_its_scale(&mut screen, &output);
    }
    let scaled = panel
        .bars()
        .into_iter()
        .map(|(_, output)| output)
        .find(|output| scale_of(output) >= 2)
        .expect("an output at scale 2");
    let before: std::collections::BTreeSet<_> =
        panel.bars().into_iter().map(|(window, _)| window).collect();
    let off = turn_output_off(&scaled).unwrap();
    wait_for(&format!("{scaled}'s bar to close"), || {
        if panel.bars().iter().any(|(_, on)| *on == scaled) {
            Err(panel.log())
        } else {
            Ok(())
        }
    });
    off.restore().unwrap();
    wait_for(&format!("a new bar on {scaled}"), || {
        panel
            .bars()
            .into_iter()
            .find(|(window, on)| *on == scaled && !before.contains(window))
            .map(|_| ())
            .ok_or_else(|| panel.log())
    });
    // The output came back as a new global, which only a client and a
    // pointer connected since name.
    let mut input = Input::connect().unwrap();
    let client = TestClient::connect().unwrap();
    let later = open_on(&mut input, &client, &scaled, "Scales", APP);
    // A capture of an output that went has stopped: capture anew.
    let mut screen = Screen::connect().unwrap();
    wait_icon_of_its_scale(&mut screen, &scaled);
    drop(later);
    two.end();
}

/// Every bar of a panel started with its outputs at two scales draws its
/// icons at its own output's scale.
#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn each_bar_draws_its_icons_at_its_outputs_scale() {
    let two = TwoScales::start();
    let mut screen = Screen::connect().unwrap();
    for (_, output) in two.panel.bars() {
        wait_icon_of_its_scale(&mut screen, &output);
    }
    two.end();
}
