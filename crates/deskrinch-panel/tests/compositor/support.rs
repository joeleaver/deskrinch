//! What the panel's compositor tests share: the panel binary started in the
//! session with the run's renderer, its settings file, and finding it on
//! the screen.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_component::{Component, Renderer};
use deskrinch_test_screen::{Image, Screen};

/// How long something that should happen gets: generous, since a loaded
/// machine runs nested compositors slowly.
pub(crate) const PATIENCE: Duration = Duration::from_secs(60);
/// How long the screen must not change to count as still.
const STILL: Duration = Duration::from_millis(500);

/// The panel binary Cargo built.
const PANEL: &str = env!("CARGO_BIN_EXE_deskrinch-panel");

/// Panics unless the tests run in a nested session started by `cargo xtask
/// compositor-tests` (`Compositor::current`): run anywhere else they would
/// put a panel on the real desktop and photograph it. Every test calls it
/// first.
pub(crate) fn in_nested_session() {
    let _compositor = Compositor::current();
}

/// The session's configuration file, in its own `XDG_CONFIG_HOME`.
fn settings_file() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME").expect("the session's XDG_CONFIG_HOME");
    PathBuf::from(config).join("deskrinch/config.toml")
}

/// Writes the configuration file whole (`text` is TOML), as an editor
/// saving it would: a new file renamed over the old.
pub(crate) fn write_settings(text: &str) {
    let path = settings_file();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let next = path.with_extension("toml.next");
    fs::write(&next, text).unwrap();
    fs::rename(&next, &path).unwrap();
}

/// The `[panel]` table placing the panel at `edge` with `height`.
pub(crate) fn panel_table(edge: Edge, height: u32) -> String {
    let edge = match edge {
        Edge::Top => "top",
        Edge::Bottom => "bottom",
    };
    format!("[panel]\nedge = \"{edge}\"\nheight = {height}\n")
}

/// Removes the configuration file: the panel's defaults.
pub(crate) fn no_settings() {
    match fs::remove_file(settings_file()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("removing the settings: {error}"),
    }
}

/// The running panel; ended when dropped.
#[derive(Debug)]
pub(crate) struct Panel {
    component: Component,
}

impl Panel {
    /// Starts the panel with the run's renderer, and returns once it has
    /// presented, having checked that renderer is what presented.
    pub(crate) fn start() -> Self {
        let renderer = Renderer::of_run();
        let component = Component::start(Command::new(PANEL), renderer).unwrap();
        if let Err(why) = component.wait_presented_with(renderer, PATIENCE) {
            panic!("the panel: {why}");
        }
        Self { component }
    }

    /// Waits until the panel has reported a mistake in its settings whose
    /// line `says` names (`panel.height`).
    pub(crate) fn wait_rejected(&self, says: &str) {
        let reported = self.component.watch().wait_for_line(PATIENCE, |line| {
            line.contains("error:") && line.contains(says)
        });
        assert!(
            reported,
            "the panel did not report {says}; its last lines:\n{}",
            self.component.watch().tail()
        );
    }
}

/// An edge of an output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    Top,
    Bottom,
}

/// Where a panel is on a captured output: along `edge`, `rows` physical
/// pixels thick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Band {
    pub(crate) edge: Edge,
    pub(crate) rows: u32,
}

impl Band {
    /// The panel's band at `edge`, `height` logical pixels at `scale`.
    pub(crate) fn of(edge: Edge, height: u32, scale: u32) -> Self {
        Self {
            edge,
            rows: height * scale,
        }
    }

    /// The image rows it covers in an image `height` rows high.
    pub(crate) fn rows_in(self, height: u32) -> std::ops::Range<u32> {
        match self.edge {
            Edge::Top => 0..self.rows,
            Edge::Bottom => height.saturating_sub(self.rows)..height,
        }
    }
}

/// Every output's picture, by name.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Pictures(pub(crate) Vec<(String, Image)>);

impl Pictures {
    /// Each output of `screen`, captured now.
    pub(crate) fn take(screen: &mut Screen) -> Self {
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

    /// Each output of `screen` once it is still: the same in two captures
    /// [`STILL`] apart. What the last test showed may still be going: KWin
    /// fades a closed window or panel out, the more slowly the busier the
    /// machine is, and a picture taken during the fade would count the
    /// whole fade as the panel's.
    pub(crate) fn still(screen: &mut Screen) -> Self {
        let deadline = Instant::now() + PATIENCE;
        let mut last = Self::take(screen);
        loop {
            std::thread::sleep(STILL);
            let now = Self::take(screen);
            if now == last {
                return now;
            }
            assert!(Instant::now() < deadline, "the screen did not keep still");
            last = now;
        }
    }
}

/// Where `after` differs from `before`: `None` when nowhere, otherwise
/// the bounding box of the changed pixels (left, top, right, bottom,
/// inclusive).
fn changed_box(before: &Image, after: &Image) -> Option<(u32, u32, u32, u32)> {
    assert_eq!(
        (before.width(), before.height()),
        (after.width(), after.height()),
        "the output changed size"
    );
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..after.height() {
        for x in 0..after.width() {
            if before.pixel(x, y) != after.pixel(x, y) {
                bounds = Some(match bounds {
                    None => (x, y, x, y),
                    Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                });
            }
        }
    }
    bounds
}

/// What is wrong with `now` against `before` for a panel at `band`, if
/// anything: exactly one output changed, and there the changed pixels'
/// bounding box is the band exactly (the full width, its first and last
/// rows). A pixel of the panel may equal what was under it, so not every
/// pixel of the band need have changed.
fn wrong(before: &Pictures, now: &Pictures, band: Band) -> Option<String> {
    let mut showing = Vec::new();
    for ((name, was), (_, is)) in before.0.iter().zip(&now.0) {
        if let Some(bounds) = changed_box(was, is) {
            showing.push((name.clone(), bounds, is.width(), is.height()));
        }
    }
    match showing.as_slice() {
        [] => Some("no output shows the panel".into()),
        [(name, bounds, width, height)] => {
            let rows = band.rows_in(*height);
            let wanted = (0, rows.start, width - 1, rows.end - 1);
            (*bounds != wanted).then(|| {
                format!(
                    "{name} changed in {bounds:?} (left, top, right, bottom), not the band \
                     {wanted:?} of a {width}x{height} output"
                )
            })
        }
        several => Some(format!(
            "more than one output changed: {:?}",
            several.iter().map(|(name, ..)| name).collect::<Vec<_>>()
        )),
    }
}

/// Captures every output until exactly one shows the panel at `band`
/// against `before`, the others unchanged; returns that output's name and
/// pictures, taken once the screen is still ([`Pictures::still`]): KWin
/// fades a panel in, and the band is already the band halfway through, but
/// not yet the panel's own pixels, which a later check compares with.
///
/// # Panics
///
/// When that is not so within [`PATIENCE`], saying how it is instead.
pub(crate) fn wait_band(screen: &mut Screen, before: &Pictures, band: Band) -> (String, Pictures) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let now = Pictures::still(screen);
        let why = wrong(before, &now, band);
        let Some(why) = why else {
            let shown = before
                .0
                .iter()
                .zip(&now.0)
                .find(|((_, was), (_, is))| was != is)
                .map(|((name, _), _)| name.clone())
                .unwrap();
            return (shown, now);
        };
        assert!(
            Instant::now() < deadline,
            "the panel is not at {band:?}: {why}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The whole scale of the output named `name`, as a client learns it.
pub(crate) fn scale_of(name: &str) -> u32 {
    let client = TestClient::connect().unwrap();
    client.output_info(name).unwrap().scale.cast_unsigned()
}

/// The scale every output has (the session sets them all alike).
pub(crate) fn session_scale(screen: &Screen) -> u32 {
    scale_of(&screen.outputs()[0])
}

#[cfg(test)]
mod tests {
    use deskrinch_test_screen::Rgba;

    use super::*;

    fn image(width: u32, height: u32, mark: &[(u32, u32)]) -> Image {
        let mut pixels = vec![Rgba::opaque(0, 0, 0); (width * height) as usize];
        for &(x, y) in mark {
            pixels[(y * width + x) as usize] = Rgba::opaque(9, 9, 9);
        }
        Image::new(width, height, pixels).unwrap()
    }

    fn pictures(images: Vec<Image>) -> Pictures {
        Pictures(
            images
                .into_iter()
                .enumerate()
                .map(|(index, image)| (format!("OUT-{index}"), image))
                .collect(),
        )
    }

    #[test]
    fn a_band_is_the_changed_box_exactly() {
        let before = pictures(vec![image(4, 6, &[]), image(4, 6, &[])]);
        let bottom = Band::of(Edge::Bottom, 2, 1);
        // Corners of the band's box changed, a pixel inside not.
        let at_bottom = pictures(vec![image(4, 6, &[(0, 4), (3, 5)]), image(4, 6, &[])]);
        assert_eq!(wrong(&before, &at_bottom, bottom), None);
        let short = pictures(vec![image(4, 6, &[(0, 5), (3, 5)]), image(4, 6, &[])]);
        assert!(wrong(&before, &short, bottom).is_some());
        let narrow = pictures(vec![image(4, 6, &[(1, 4), (3, 5)]), image(4, 6, &[])]);
        assert!(wrong(&before, &narrow, bottom).is_some());
        let stray = pictures(vec![
            image(4, 6, &[(0, 4), (3, 5), (2, 1)]),
            image(4, 6, &[]),
        ]);
        assert!(wrong(&before, &stray, bottom).is_some());
    }

    #[test]
    fn exactly_one_output_shows_it() {
        let before = pictures(vec![image(4, 6, &[]), image(4, 6, &[])]);
        let top = Band::of(Edge::Top, 1, 2);
        assert_eq!(top.rows_in(6), 0..2);
        let neither = pictures(vec![image(4, 6, &[]), image(4, 6, &[])]);
        assert!(wrong(&before, &neither, top).unwrap().contains("no output"));
        let both = pictures(vec![
            image(4, 6, &[(0, 0), (3, 1)]),
            image(4, 6, &[(0, 0), (3, 1)]),
        ]);
        assert!(
            wrong(&before, &both, top)
                .unwrap()
                .contains("more than one")
        );
        let second = pictures(vec![image(4, 6, &[]), image(4, 6, &[(0, 0), (3, 1)])]);
        assert_eq!(wrong(&before, &second, top), None);
    }
}
