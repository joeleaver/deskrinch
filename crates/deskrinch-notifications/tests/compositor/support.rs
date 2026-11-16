//! What the daemon's compositor tests share: the daemon started in the
//! session with the run's renderer and an icon theme of the test's own, a
//! client sending to it on the session's bus, and the column found on the
//! screen.
//!
//! The column is found by what changed: every output is captured, still,
//! before the daemon shows anything, and the column is the pixels that
//! differ since. The popups are the runs of rows that differ inside it,
//! apart from each other by the column's gaps, which paint nothing.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use deskrinch_notification_service::NAME;
use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_component::{Component, Renderer, programs};
use deskrinch_test_input::{Button, Input};
use deskrinch_test_notify::{Client, Signal};
use deskrinch_test_screen::{Image, Rgba, Screen};
use deskrinch_test_wait::{Deadline, comes_true, wait_for};
use tempfile::TempDir;
use zbus::Variant;
use zbus::fdo::DBusProxy;

/// How long the screen must not change to count as still: longer than the
/// longest close animation KWin runs by default (its fading popups, 600 ms);
/// sway and labwc animate nothing.
const STILL: Duration = Duration::from_millis(600);

/// The colour of the test's icon for an action, `test-red`.
pub(crate) const RED_ICON: Rgba = Rgba::opaque(220, 30, 30);

/// The name of that icon in the test's theme.
pub(crate) const RED_ICON_NAME: &str = "test-red";

/// How far a colour on the screen may be from the one looked for, per
/// channel: a compositor's rounding, far less than between two colours.
const TOLERANCE: u8 = 8;

/// The theme's spacing the column is padded with (`--rinch-spacing-sm`),
/// in logical pixels: the popups start this far from the output's edges.
pub(crate) const PADDING: u32 = 12;

/// The column's width in logical pixels, its padding included: the
/// daemon's.
pub(crate) const WIDTH: u32 = 380;

/// Panics unless the tests run in a nested session started by `cargo xtask
/// compositor-tests`: run anywhere else they would take the real session's
/// notifications and photograph its screen. Every test calls it first.
fn in_nested_session() {
    let _compositor = Compositor::current();
}

/// Each output's picture, by name.
#[derive(Debug, PartialEq, Eq)]
struct Pictures(Vec<(String, Image)>);

impl Pictures {
    fn take(screen: &mut Screen) -> Self {
        Self(
            screen
                .outputs()
                .into_iter()
                .map(|name| {
                    let image = screen.capture(&name).unwrap();
                    (name, image)
                })
                .collect(),
        )
    }

    /// Each output once it is still: the same in two captures [`STILL`]
    /// apart.
    fn still(screen: &mut Screen) -> Self {
        let deadline = Deadline::patience();
        let mut last = Self::take(screen);
        loop {
            std::thread::sleep(STILL);
            let now = Self::take(screen);
            if now == last {
                return now;
            }
            assert!(!deadline.passed(), "the screen never stood still");
            last = now;
        }
    }
}

/// The column on the screen: the output it is on and the box of what it
/// paints there, in the screen's pixels, inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) output: String,
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
    /// Each popup's first and last rows, top to bottom.
    pub(crate) popups: Vec<(u32, u32)>,
    /// The output's size in its pixels.
    pub(crate) size: (u32, u32),
    /// What changed, coarsely, for a failure to show: every eighth row
    /// that changed, with the spans of its columns that did.
    pub(crate) map: Vec<String>,
}

/// The spans of `row`'s pixels that differ between `before` and `now`, as
/// `"y: x0-x1 x2-x3"`; `None` when none does.
fn spans(before: &Image, now: &Image, row: u32) -> Option<String> {
    let mut spans: Vec<(u32, u32)> = Vec::new();
    for x in 0..now.width() {
        if before.pixel(x, row) != now.pixel(x, row) {
            match spans.last_mut() {
                Some((_, end)) if *end + 1 == x => *end = x,
                _ => spans.push((x, x)),
            }
        }
    }
    (!spans.is_empty()).then(|| {
        let spans: Vec<String> = spans.iter().map(|(a, b)| format!("{a}-{b}")).collect();
        format!("{row}: {}", spans.join(" "))
    })
}

/// The column on the output `output`, which showed `before` and shows `now`:
/// the box of the pixels that differ, and the runs of rows that hold any;
/// `None` when none differs.
fn changed(output: &str, before: &Image, now: &Image) -> Option<Column> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for y in 0..now.height() {
        let mut any = false;
        for x in 0..now.width() {
            if before.pixel(x, y) != now.pixel(x, y) {
                any = true;
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                });
            }
        }
        if any {
            match runs.last_mut() {
                Some((_, end)) if *end + 1 == y => *end = y,
                _ => runs.push((y, y)),
            }
        }
    }
    bounds.map(|(left, top, right, bottom)| Column {
        output: output.to_owned(),
        left,
        top,
        right,
        bottom,
        popups: runs,
        size: (now.width(), now.height()),
        map: (0..now.height())
            .step_by(8)
            .filter_map(|row| spans(before, now, row))
            .collect(),
    })
}

/// Whether `colour` is within [`TOLERANCE`] of `wanted`.
pub(crate) fn near(colour: Rgba, wanted: Rgba) -> bool {
    colour.r.abs_diff(wanted.r) <= TOLERANCE
        && colour.g.abs_diff(wanted.g) <= TOLERANCE
        && colour.b.abs_diff(wanted.b) <= TOLERANCE
}

/// Whether `colour` is clearly red: the theme's red border, whatever its
/// shade.
pub(crate) fn reddish(colour: Rgba) -> bool {
    colour.r >= 180 && colour.g <= 120 && colour.b <= 120
}

/// The daemon in a session, a client of it, and the screen and pointer.
pub(crate) struct Notifications {
    _data: TempDir,
    daemon: Component,
    pub(crate) client: Client,
    screen: Screen,
    input: Input,
    before: Pictures,
    /// The session's scale: the screen's pixels per logical pixel.
    pub(crate) scale: u32,
}

impl Notifications {
    /// The pointer, then the daemon with the test's icon theme, once it
    /// owns the name; nothing shown yet.
    pub(crate) fn start() -> Self {
        in_nested_session();
        let input = Input::connect().unwrap();
        let data = tempfile::tempdir().unwrap();
        install(data.path());
        let mut screen = Screen::connect().unwrap();
        // Out of the way of the column, so that nothing is under it.
        let first = screen.outputs()[0].clone();
        let mut input = input;
        input.move_pointer(&first, 1.0, 1.0).ok();
        let before = Pictures::still(&mut screen);
        let mut command =
            Command::new(programs::program(deskrinch_shell::NOTIFICATIONS.package).unwrap());
        command.env("XDG_DATA_HOME", data.path());
        // A panic of the daemon's shows where, in the failure's lines.
        command.env("RUST_BACKTRACE", "1");
        let daemon = Component::start(command, Renderer::of_run()).unwrap();
        let connection = zbus::block_on(zbus::Connection::session()).unwrap();
        let bus = zbus::block_on(DBusProxy::new(&connection)).unwrap();
        let owned = comes_true(|| {
            zbus::block_on(bus.get_connection_unix_process_id(NAME.try_into().unwrap()))
                .is_ok_and(|pid| pid == daemon.id())
        });
        assert!(
            owned,
            "the daemon did not take {NAME}; its last lines:\n{}",
            daemon.watch().tail()
        );
        let client = Client::on(&connection);
        let scale = TestClient::connect()
            .unwrap()
            .output_info(&first)
            .unwrap()
            .scale
            .cast_unsigned();
        Self {
            _data: data,
            daemon,
            client,
            screen,
            input,
            before,
            scale,
        }
    }

    /// Sends a notification from "Test" with `actions` and `hints`.
    pub(crate) fn send(
        &self,
        replaces: u32,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        timeout: i32,
    ) -> u32 {
        self.client
            .send(replaces, summary, body, actions, hints, timeout)
            .unwrap()
    }

    /// The daemon's component, for its frames and lines.
    pub(crate) fn daemon(&self) -> &Component {
        &self.daemon
    }

    /// Waits until the column on the screen is one `accept` takes, and
    /// returns it.
    pub(crate) fn column(&mut self, what: &str, accept: impl Fn(&Column) -> bool) -> Column {
        wait_for(what, || {
            let now = Pictures::still(&mut self.screen);
            let columns: Vec<Column> = self
                .before
                .0
                .iter()
                .zip(&now.0)
                .filter_map(|((name, was), (_, is))| changed(name, was, is))
                .collect();
            match columns.as_slice() {
                [column] if accept(column) => Ok(column.clone()),
                _ => Err((columns, self.said())),
            }
        })
    }

    /// What the daemon said, for a failure: from its panic on, whole, when
    /// it panicked (a backtrace is longer than a tail); else its last
    /// lines.
    pub(crate) fn said(&self) -> String {
        let lines = self.daemon.watch().lines();
        match lines.iter().position(|line| line.contains("panicked at")) {
            Some(at) => lines[at..].join("\n"),
            None => self.daemon.watch().tail(),
        }
    }

    /// Waits until the screen is as it was before the daemon showed
    /// anything.
    pub(crate) fn wait_gone(&mut self) {
        let before = &self.before;
        let screen = &mut self.screen;
        wait_for("the screen as it was", || {
            let now = Pictures::still(screen);
            if now == *before {
                Ok(())
            } else {
                Err("it differs")
            }
        });
    }

    /// The image of `column`'s output now.
    pub(crate) fn picture(&mut self, column: &Column) -> Image {
        self.screen.capture(&column.output).unwrap()
    }

    /// Moves the pointer to `(x, y)`, the screen's pixels on `column`'s
    /// output.
    pub(crate) fn point(&mut self, column: &Column, x: u32, y: u32) {
        let scale = f64::from(self.scale);
        let (x, y) = ((f64::from(x) + 0.5) / scale, (f64::from(y) + 0.5) / scale);
        self.input.move_pointer(&column.output, x, y).unwrap();
    }

    /// Clicks the primary button at `(x, y)`, as [`point`](Self::point).
    pub(crate) fn click(&mut self, column: &Column, x: u32, y: u32) {
        self.point(column, x, y);
        self.input.click(Button::PRIMARY).unwrap();
    }

    /// Moves the pointer off the column, to the output's top-left corner.
    pub(crate) fn point_away(&mut self, column: &Column) {
        self.input.move_pointer(&column.output, 1.0, 1.0).unwrap();
    }

    /// Whether `column` sits in its output's top-right corner: its right and
    /// top edges within the padding (and a shadow's few pixels) of the
    /// output's, and no wider than the column.
    pub(crate) fn top_right(&self, column: &Column) -> bool {
        let (width, _) = column.size;
        let reach = (PADDING + 4) * self.scale;
        column.right + 1 + reach >= width
            && column.top <= reach
            && column.left + WIDTH * self.scale >= width
    }

    /// The next signal the client gets that is not an activation token.
    pub(crate) fn next_signal(&self) -> Signal {
        loop {
            match self.client.signals.next() {
                Signal::ActivationToken(..) => {}
                other => return other,
            }
        }
    }
}

/// The box of the pixels near `colour` in rows `rows` of `image`, if any.
pub(crate) fn find(image: &Image, rows: (u32, u32), colour: Rgba) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in rows.0..=rows.1 {
        for x in 0..image.width() {
            if image.pixel(x, y).is_some_and(|p| near(p, colour)) {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                });
            }
        }
    }
    bounds
}

/// Where a popup's close button is: the rightmost mark in its header, a
/// band at its top, other than its border; `(x, y)` in the screen's pixels.
pub(crate) fn close_button(
    image: &Image,
    column: &Column,
    popup: (u32, u32),
    scale: u32,
) -> (u32, u32) {
    let (top, _) = popup;
    // The popup's own colour, inside its border, at the header's left.
    let inner = column.left + 4 * scale;
    let ground = image.pixel(inner, top + 4 * scale).unwrap();
    let band = (top + 6 * scale, top + 40 * scale);
    let edge = column.right.saturating_sub(4 * scale);
    let mut rightmost: Option<(u32, u32)> = None;
    for y in band.0..=band.1 {
        for x in (inner..edge).rev() {
            let p = image.pixel(x, y).unwrap();
            let differs = p.r.abs_diff(ground.r) > 40
                || p.g.abs_diff(ground.g) > 40
                || p.b.abs_diff(ground.b) > 40;
            if differs {
                if rightmost.is_none_or(|(rx, _)| x > rx) {
                    rightmost = Some((x, y));
                }
                break;
            }
        }
    }
    let (x, y) = rightmost.expect("a mark in the popup's header");
    // The mark's middle: a few of its pixels left of its right edge.
    (x.saturating_sub(2 * scale), y + 2 * scale)
}

/// Installs the test's icons under `data`: a `hicolor` theme, which every
/// theme's lookups end in, with [`RED_ICON_NAME`] in [`RED_ICON`].
fn install(data: &Path) {
    let icons = data.join("icons/hicolor/scalable/actions");
    fs::create_dir_all(&icons).unwrap();
    fs::write(data.join("icons/hicolor/index.theme"), HICOLOR).unwrap();
    let Rgba { r, g, b, .. } = RED_ICON;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" \
         viewBox=\"0 0 16 16\"><rect width=\"16\" height=\"16\" \
         fill=\"#{r:02x}{g:02x}{b:02x}\"/></svg>\n"
    );
    fs::write(icons.join(format!("{RED_ICON_NAME}.svg")), svg).unwrap();
}

/// The test's `hicolor`: one directory of scalable action icons.
const HICOLOR: &str = "[Icon Theme]\nName=Hicolor\nDirectories=scalable/actions\n\n\
                       [scalable/actions]\nSize=16\nMinSize=8\nMaxSize=512\nType=Scalable\n";
