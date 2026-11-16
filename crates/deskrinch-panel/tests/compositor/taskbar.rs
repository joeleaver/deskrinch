//! The taskbar's fixture: a panel whose applications are the test's own,
//! each with an icon of one solid colour, and windows of those
//! applications; the buttons are found on the screen by their icons'
//! colours, and clicked through `deskrinch-test-input`'s pointer. The tests
//! are [`crate::taskbar_clicks`].
//!
//! The panel is started with an `XDG_DATA_HOME` of the test's own, which
//! holds the entries and a `hicolor` theme with their icons: whichever icon
//! theme the panel picks, its lookups end in `hicolor`, and these names are
//! in no other theme. The keyboard and pointer are made before any window,
//! since on a headless wlroots seat they are the seat's only ones, and a
//! window mapped without a keyboard gets no keyboard focus.

use std::fmt;
use std::fs;
use std::path::Path;

use deskrinch_test_client::TestClient;
use deskrinch_test_input::{Button, Input};
use deskrinch_test_screen::{Image, Rgba, Screen};
use deskrinch_test_wait::{recv, wait_for};
use deskrinch_window_management::{Event, WindowId, WindowManager};
use tempfile::TempDir;

use crate::support::{
    Band, Edge, Panel, Pictures, STILL, in_nested_session, panel_table, scale_of, wait_band,
    write_settings,
};

/// The panel's height, in logical pixels.
const HEIGHT: u32 = 40;
/// A button's icon's side, in logical pixels: the panel's.
const ICON: u32 = 16;
/// The opacity a minimized window's button is drawn with: the panel's.
const DIMMED: f32 = 0.55;
/// How far a colour on the screen may be from the icon's, per channel:
/// enough for a compositor's rounding, far less than between two icons.
const TOLERANCE: u8 = 8;

/// One of the test's applications: its id (its entry's file name and its
/// windows' app id) and its icon's colour.
#[derive(Debug, Clone, Copy)]
pub(crate) struct App {
    pub(crate) id: &'static str,
    pub(crate) colour: Rgba,
}

pub(crate) const RED: App = App {
    id: "org.deskrinch.taskbar.Red",
    colour: Rgba::opaque(220, 30, 30),
};
pub(crate) const GREEN: App = App {
    id: "org.deskrinch.taskbar.Green",
    colour: Rgba::opaque(30, 190, 60),
};
pub(crate) const MAGENTA: App = App {
    id: "org.deskrinch.taskbar.Magenta",
    colour: Rgba::opaque(200, 40, 200),
};
const APPS: [App; 3] = [RED, GREEN, MAGENTA];

/// A panel with the test's applications, the screen it is on, the pointer
/// to click it with, and a client to open windows with.
pub(crate) struct Bar {
    _dirs: TempDir,
    pub(crate) input: Input,
    screen: Screen,
    pub(crate) client: TestClient,
    panel: Panel,
    /// The output the panel is on.
    output: String,
    /// That output's scale: the screen's pixels per logical pixel.
    scale: u32,
    band: Band,
}

impl Bar {
    /// The pointer and keyboard, then the panel at the bottom with the
    /// test's applications, once it is on the screen.
    pub(crate) fn start() -> Self {
        Self::start_with("")
    }

    /// [`start`](Self::start), in the colour scheme `scheme` (`"dark"` or
    /// `"light"`, the `[general]` `color-scheme` setting).
    pub(crate) fn start_in(scheme: &str) -> Self {
        Self::start_with(&format!("[general]\ncolor-scheme = \"{scheme}\"\n"))
    }

    /// [`start`](Self::start), with `more` settings beside the panel's.
    fn start_with(more: &str) -> Self {
        in_nested_session();
        let input = Input::connect().unwrap();
        write_settings(&format!("{more}{}", panel_table(Edge::Bottom, HEIGHT)));
        let dirs = tempfile::tempdir().unwrap();
        install(dirs.path());
        let mut screen = Screen::connect().unwrap();
        let before = Pictures::still(&mut screen);
        let mut command = Panel::command();
        command.env("XDG_DATA_HOME", dirs.path());
        let panel = Panel::start_with(command);
        // The scale is the same on every output of a pass.
        let scale = scale_of(&screen.outputs()[0]);
        let band = Band::of(Edge::Bottom, HEIGHT, scale);
        let (output, _) = wait_band(&mut screen, &before, band);
        Self {
            _dirs: dirs,
            input,
            screen,
            client: TestClient::connect().unwrap(),
            panel,
            output,
            scale,
            band,
        }
    }

    /// Captures the panel's output until `accept` takes the picture;
    /// panics, with what the panel logged, when none comes.
    pub(crate) fn see(&mut self, what: &str, accept: impl Fn(&Self, &Image) -> bool) -> Image {
        let output = self.output.clone();
        wait_for(what, || {
            let image = self.screen.capture(&output).unwrap();
            if accept(self, &image) {
                Ok(image)
            } else {
                Err(Unseen {
                    output: output.clone(),
                    log: self.panel.log(),
                })
            }
        })
    }

    /// `part` of the panel's output once it is still: the same in two
    /// captures [`STILL`] apart. Nothing reports the end of a change the
    /// compositor animates, so two pictures alike are the only sign of it.
    pub(crate) fn settled<T: PartialEq>(&mut self, part: impl Fn(&Image) -> T) -> T {
        let output = self.output.clone();
        let mut last = part(&self.screen.capture(&output).unwrap());
        wait_for(&format!("{output} to keep still"), || {
            std::thread::sleep(STILL);
            let now = part(&self.screen.capture(&output).unwrap());
            if now == last {
                Ok(now)
            } else {
                last = now;
                Err("changed again")
            }
        })
    }

    /// Where `app`'s button's icon is drawn at full colour in `image`:
    /// the box of its pixels (left, top, right, bottom, inclusive), when
    /// there are enough of them to be a whole icon.
    pub(crate) fn icon(&self, image: &Image, app: App) -> Option<(u32, u32, u32, u32)> {
        let mut bounds: Option<(u32, u32, u32, u32)> = None;
        let mut count = 0_u32;
        for y in self.band.rows_in(image.height()) {
            for x in 0..image.width() {
                if image.pixel(x, y).is_some_and(|p| near(p, app.colour)) {
                    count += 1;
                    bounds = Some(match bounds {
                        None => (x, y, x, y),
                        Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                    });
                }
            }
        }
        let side = ICON * self.scale;
        // Its edges may be blended with what is around it.
        (count >= (side - 2) * (side - 2))
            .then_some(bounds)
            .flatten()
    }

    /// How many buttons of `app` `image` shows: icons side by side.
    pub(crate) fn icons(&self, image: &Image, app: App) -> usize {
        let Some((left, top, right, bottom)) = self.icon(image, app) else {
            return 0;
        };
        let row = top + (bottom - top) / 2;
        let mut count = 0;
        let mut inside = false;
        for x in left..=right {
            let now = image.pixel(x, row).is_some_and(|p| near(p, app.colour));
            if now && !inside {
                count += 1;
            }
            inside = now;
        }
        count
    }

    /// Whether `app`'s one button is drawn dimmed in `image`: its icon at
    /// [`DIMMED`] over the panel, at the middle of `was`, where it was drawn
    /// at full colour.
    pub(crate) fn dimmed(image: &Image, app: App, was: (u32, u32, u32, u32)) -> bool {
        let (left, top, right, bottom) = was;
        let middle = (left + (right - left) / 2, top + (bottom - top) / 2);
        // Inside the button's padding, left of the icon: the panel, since
        // a minimized window is not the active one.
        let Some(under) = image.pixel(left.saturating_sub(2), middle.1) else {
            return false;
        };
        let expected = blend(app.colour, under, DIMMED);
        image
            .pixel(middle.0, middle.1)
            .is_some_and(|p| near(p, expected))
    }

    /// Waits until `app`'s button is drawn at full colour, and returns its
    /// icon's box.
    pub(crate) fn wait_button(&mut self, app: App) -> (u32, u32, u32, u32) {
        let image = self.see(&format!("{}'s button", app.id), |bar, image| {
            bar.icon(image, app).is_some()
        });
        self.icon(&image, app).unwrap()
    }

    /// Clicks `button` on the middle of `icon`, a box in the screen's
    /// pixels.
    pub(crate) fn click(&mut self, icon: (u32, u32, u32, u32), button: Button) {
        self.point_at(icon);
        self.input.click(button).unwrap();
    }

    /// Moves the pointer onto the middle of `icon`, a box in the screen's
    /// pixels: onto its button, over its icon.
    pub(crate) fn point_at(&mut self, icon: (u32, u32, u32, u32)) {
        let (left, top, right, bottom) = icon;
        let scale = f64::from(self.scale);
        let x = f64::from(left + right + 1) / 2.0 / scale;
        let y = f64::from(top + bottom + 1) / 2.0 / scale;
        self.input.move_pointer(&self.output, x, y).unwrap();
    }

    /// Moves the pointer off the panel, to the middle of its output.
    pub(crate) fn point_off(&mut self) {
        let image = self.screen.capture(&self.output.clone()).unwrap();
        let scale = f64::from(self.scale);
        let x = f64::from(image.width()) / 2.0 / scale;
        let y = f64::from(image.height()) / 2.0 / scale;
        self.input.move_pointer(&self.output, x, y).unwrap();
    }

    /// The background of the button whose icon is `icon`: inside its
    /// padding, left of the icon.
    pub(crate) fn beside(image: &Image, icon: (u32, u32, u32, u32)) -> Option<Rgba> {
        let (left, top, _, bottom) = icon;
        image.pixel(left.saturating_sub(2), u32::midpoint(top, bottom))
    }

    /// The bar's own colour, in the row of `icon`: at its left end, in the
    /// bar's padding before the taskbar, where no button reaches on any
    /// output however narrow.
    pub(crate) fn bar(image: &Image, icon: (u32, u32, u32, u32)) -> Option<Rgba> {
        let (_, top, _, bottom) = icon;
        image.pixel(1, u32::midpoint(top, bottom))
    }

    /// The panel, for its log.
    /// The output the bar under test is on.
    pub(crate) fn output(&self) -> &str {
        &self.output
    }

    pub(crate) fn panel(&self) -> &Panel {
        &self.panel
    }

    /// The output's width in logical pixels.
    pub(crate) fn logical_width(&mut self) -> u32 {
        let image = self.screen.capture(&self.output.clone()).unwrap();
        image.width() / self.scale
    }
}

/// The window with `app_id`, once the compositor lists it, and a manager
/// to act on it with: on a connection of the test's own, as another
/// window-management client would.
pub(crate) fn window_of(app_id: &str) -> (WindowManager, WindowId) {
    let (sender, events) = std::sync::mpsc::channel();
    let wanted = app_id.to_owned();
    let manager = deskrinch_window_management::connect(move |event| {
        if let Event::Added(window) | Event::Changed(window) = event
            && window.app_id == wanted
        {
            // The receiver may be gone once the window is found.
            let _ = sender.send(window.id);
        }
    })
    .expect("connect to the compositor's window management");
    let id = recv(&format!("the window of {app_id}"), &events);
    (manager, id)
}

/// What a capture that was not yet what a test waited for leaves to show:
/// the output looked at, and what the panel logged.
struct Unseen {
    output: String,
    log: String,
}

impl fmt::Debug for Unseen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not on {}; the panel logged:\n{}", self.output, self.log)
    }
}

/// Whether `a` is within [`TOLERANCE`] of `b` in each colour channel.
pub(crate) fn near(a: Rgba, b: Rgba) -> bool {
    [(a.r, b.r), (a.g, b.g), (a.b, b.b)]
        .into_iter()
        .all(|(a, b)| a.abs_diff(b) <= TOLERANCE)
}

/// `over` drawn with `opacity` on `under`.
fn blend(over: Rgba, under: Rgba, opacity: f32) -> Rgba {
    let mix = |o: u8, u: u8| {
        let mixed = f32::from(o) * opacity + f32::from(u) * (1.0 - opacity);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "rounded, and within a channel's range by construction"
        )]
        let channel = mixed.round() as u8;
        channel
    };
    Rgba::opaque(
        mix(over.r, under.r),
        mix(over.g, under.g),
        mix(over.b, under.b),
    )
}

/// Writes the test's applications into the data directory `data`: an
/// entry each, and its icon, a square of its colour, in `hicolor`.
fn install(data: &Path) {
    let applications = data.join("applications");
    let theme = data.join("icons/hicolor");
    let icons = theme.join("scalable/apps");
    fs::create_dir_all(&applications).unwrap();
    fs::create_dir_all(&icons).unwrap();
    fs::write(theme.join("index.theme"), HICOLOR).unwrap();
    for app in APPS {
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName={id}\nExec=/bin/false\nIcon={id}\n",
            id = app.id
        );
        fs::write(applications.join(format!("{}.desktop", app.id)), entry).unwrap();
        let Rgba { r, g, b, .. } = app.colour;
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" \
             viewBox=\"0 0 16 16\"><rect width=\"16\" height=\"16\" \
             fill=\"#{r:02x}{g:02x}{b:02x}\"/></svg>\n"
        );
        fs::write(icons.join(format!("{}.svg", app.id)), svg).unwrap();
    }
}

/// The test's `hicolor`: one directory of scalable application icons.
const HICOLOR: &str = "[Icon Theme]\nName=Hicolor\nDirectories=scalable/apps\n\n\
                       [scalable/apps]\nSize=16\nMinSize=8\nMaxSize=512\nType=Scalable\n";
