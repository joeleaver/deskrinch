//! What the wallpaper's compositor tests share: the wallpaper started in
//! the session with the run's renderer, its settings file, and reading its
//! colour off the screen.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use deskrinch_test_client::Compositor;
use deskrinch_test_component::{Component, Mark, Renderer, log};
use deskrinch_test_screen::{Image, Rgba, Screen};

/// How long something that should happen gets: generous, since a loaded
/// machine runs nested compositors slowly.
pub(crate) const PATIENCE: Duration = Duration::from_secs(60);

/// The wallpaper binary Cargo built.
const WALLPAPER: &str = env!("CARGO_BIN_EXE_deskrinch-wallpaper");

/// A colour no compositor's own background has.
pub(crate) const BLUE: Rgba = Rgba::opaque(0x24, 0x5c, 0xd6);
/// Another.
pub(crate) const ORANGE: Rgba = Rgba::opaque(0xe0, 0x7a, 0x1f);

/// Panics unless the tests run in a nested session started by `cargo xtask
/// compositor-tests` (`Compositor::current`): run anywhere else they would
/// put a wallpaper on the real desktop and photograph it. Every test calls
/// it first.
pub(crate) fn in_nested_session() -> Compositor {
    Compositor::current()
}

/// `color` as the settings write it.
pub(crate) fn written(color: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
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

/// The running wallpaper; ended when dropped.
#[derive(Debug)]
pub(crate) struct Wallpaper {
    component: Component,
}

impl Wallpaper {
    /// Starts the wallpaper with the run's renderer, and returns once each
    /// of the session's outputs has a window that has presented its first
    /// frame (rinch's per-window frame lines, `[PERF] window <id> frame N:`),
    /// having checked that the run's renderer presented them. What the
    /// windows show is the screen's to tell.
    pub(crate) fn start() -> Self {
        let renderer = Renderer::of_run();
        let component = Component::start(Command::new(WALLPAPER), renderer).unwrap();
        let watch = component.watch();
        let said = watch.wait_for_line(PATIENCE, |line| {
            log::presents_with_software(line) || log::adapter(line).is_some()
        });
        assert!(
            said,
            "the wallpaper never said what it presents with; its last lines:\n{}",
            watch.tail()
        );
        let as_asked = match renderer {
            Renderer::Software => watch.presents_with_software(),
            Renderer::Gpu => {
                !watch.presents_with_software()
                    && watch
                        .adapter()
                        .is_some_and(|adapter| !adapter.is_software())
            }
        };
        assert!(
            as_asked,
            "the wallpaper was to present with the {} renderer; its last lines:\n{}",
            renderer.env(),
            watch.tail()
        );
        let outputs = Screen::connect().unwrap().outputs().len();
        assert!(
            watch.wait_first_frames(outputs, PATIENCE).is_some(),
            "the wallpaper did not present a frame in each of {outputs} windows; its windows: \
             {:?}",
            watch.windows()
        );
        Self { component }
    }

    /// Every window's frames now, to count from.
    pub(crate) fn mark(&self) -> Mark {
        self.component.watch().mark()
    }

    /// Waits until each of the wallpaper's open windows has presented a frame
    /// since `mark`.
    pub(crate) fn wait_each_window_drew_since(&self, mark: &Mark) {
        let watch = self.component.watch();
        let deadline = Instant::now() + PATIENCE;
        loop {
            let since = watch.since(mark);
            let drew = watch.windows().iter().all(|open| {
                since
                    .iter()
                    .any(|s| s.window == open.window && s.frames > 0)
            });
            if drew {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "not every window drew since the mark: {since:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the wallpaper has said something in which `says` is.
    pub(crate) fn wait_said(&self, says: &str) {
        let said = self
            .component
            .watch()
            .wait_for_line(PATIENCE, |line| line.contains(says));
        assert!(
            said,
            "the wallpaper did not say {says:?}; its last lines:\n{}",
            self.component.watch().tail()
        );
    }

    /// Fails if the wallpaper has logged an error so far.
    pub(crate) fn assert_no_error(&self) {
        let errored = self
            .component
            .watch()
            .wait_for_line(Duration::ZERO, log::is_error);
        assert!(
            !errored,
            "the wallpaper logged an error; its last lines:\n{}",
            self.component.watch().tail()
        );
    }

    /// Whether it still runs.
    pub(crate) fn running(&mut self) -> bool {
        matches!(self.component.wait_exit(Duration::ZERO), Ok(None))
    }
}

/// Whether `pixel` is `color`, within what a compositor's blending and
/// colour conversion may change: 2 in each channel.
pub(crate) fn about(pixel: Rgba, color: Rgba) -> bool {
    let near = |a: u8, b: u8| a.abs_diff(b) <= 2;
    near(pixel.r, color.r) && near(pixel.g, color.g) && near(pixel.b, color.b)
}

/// The first pixel of `image` that is not `color`, if any, with where it is.
pub(crate) fn first_other(image: &Image, color: Rgba) -> Option<(u32, u32, Rgba)> {
    first_other_within(image, color, 0)
}

/// [`first_other`], leaving out the pixels within `margin` of the image's
/// edges.
pub(crate) fn first_other_within(
    image: &Image,
    color: Rgba,
    margin: u32,
) -> Option<(u32, u32, Rgba)> {
    let (width, height) = (image.width(), image.height());
    (margin..height.saturating_sub(margin)).find_map(|y| {
        (margin..width.saturating_sub(margin)).find_map(|x| {
            let pixel = image.pixel(x, y).unwrap();
            (!about(pixel, color)).then_some((x, y, pixel))
        })
    })
}

/// Waits until every output of the session is `color` from edge to edge,
/// as captured afresh; panics with the first output and pixel that is not.
pub(crate) fn wait_covered(color: Rgba) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let mut screen = Screen::connect().unwrap();
        let wrong = screen.outputs().into_iter().find_map(|name| {
            let image = screen.capture(&name).unwrap();
            first_other(&image, color).map(|at| (name, image.width(), image.height(), at))
        });
        let Some((name, width, height, (x, y, pixel))) = wrong else {
            return;
        };
        assert!(
            Instant::now() < deadline,
            "{name} ({width}x{height}) is not all {color:?}: {pixel:?} at ({x}, {y})"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Waits until `check` finds nothing wrong with the output `name`'s
/// picture, captured afresh; panics with what it last found.
pub(crate) fn wait_output(name: &str, check: impl Fn(&Image) -> Option<String>) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let mut screen = Screen::connect().unwrap();
        let image = screen.capture(name).unwrap();
        let Some(why) = check(&image) else {
            return;
        };
        assert!(Instant::now() < deadline, "{name}: {why}");
        std::thread::sleep(Duration::from_millis(200));
    }
}
