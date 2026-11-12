//! The launcher as the user meets it: keys typed into it (through
//! `deskrinch-test-input`), what it draws (captured with
//! `deskrinch-test-screen`), and what it starts.
//!
//! Each test runs a launcher of its own with directories of its own: the
//! only applications it finds are the test's, recorders (`/bin/sh` on a
//! script) that write what they were started with, and its configuration
//! is the test's. The keyboard is made before the launcher, since on a
//! headless wlroots seat it is the seat's only one, and a launcher that maps
//! without one gets no keyboard focus. Clicks are [`crate::click`]'s, on this
//! fixture.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use deskrinch_test_input::{Input, Key};
use deskrinch_test_screen::{Image, Rgba, Screen};
use tempfile::TempDir;

use crate::support::{Launcher, PATIENCE, client, eventually, in_nested_session, launcher, stays};

/// The launcher's size on the screen (scale 1): its layer surface's.
pub(crate) const WIDTH: u32 = 640;
pub(crate) const HEIGHT: u32 = 420;
/// The column the bands are read along: inside the launcher's padding, and
/// left of a result's icon and name.
pub(crate) const COLUMN: u32 = 22;

/// A launcher shown, with the keyboard to type into it and the screen to
/// look at it.
pub(crate) struct Shown {
    dirs: TempDir,
    pub(crate) input: Input,
    screen: Screen,
    /// The output the launcher is drawn on.
    pub(crate) output: String,
    pub(crate) launcher: Launcher,
}

impl Shown {
    /// The keyboard, then a launcher run with `--show` on the test's
    /// applications and configuration (`config`, as `config.toml`), once it
    /// is on the screen.
    pub(crate) fn start(config: &str) -> Self {
        in_nested_session();
        let input = Input::connect().unwrap();
        let screen = Screen::connect().unwrap();
        let dirs = tempfile::tempdir().unwrap();
        let recorder = dirs.path().join("record.sh");
        fs::write(&recorder, RECORDER).unwrap();
        fs::create_dir(dirs.path().join("started")).unwrap();
        let applications = dirs.path().join("data/applications");
        fs::create_dir_all(&applications).unwrap();
        for (id, name, terminal) in [
            ("Plain", "Plain Recorder", false),
            ("Term", "Terminal Recorder", true),
            // A second result for "plain", below the first: a word of its
            // name starts with it, where the first's name does.
            ("Other", "Recorder Plain", false),
        ] {
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec=/bin/sh {} {id}\nTerminal={terminal}\n",
                recorder.display()
            );
            fs::write(
                applications.join(format!("org.deskrinch.{id}.desktop")),
                entry,
            )
            .unwrap();
        }
        let config = config.replace("{recorder}", &recorder.display().to_string());
        fs::create_dir_all(dirs.path().join("config/deskrinch")).unwrap();
        fs::write(dirs.path().join("config/deskrinch/config.toml"), config).unwrap();
        let launcher = Launcher::spawn(in_dirs(launcher(&["--show"]), dirs.path()));
        let mut shown = Self {
            dirs,
            input,
            screen,
            output: String::new(),
            launcher,
        };
        shown.launcher.wait_presented();
        shown.output = shown.find_output();
        shown
    }

    /// The output the launcher is drawn on, once it is: the compositor
    /// chooses, and the session has more than one.
    fn find_output(&mut self) -> String {
        let outputs = self.screen.outputs();
        let deadline = Instant::now() + PATIENCE;
        loop {
            for output in &outputs {
                let image = self.screen.capture(output).unwrap();
                if at_centre(&image).is_some() {
                    return output.clone();
                }
            }
            assert!(
                Instant::now() < deadline,
                "the launcher was not drawn on any of {outputs:?}\nthe launcher logged:\n{}",
                self.launcher.log()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Captures until `accept` takes an image; panics, with what the
    /// launcher logged, when none comes.
    pub(crate) fn see(&mut self, what: &str, accept: impl Fn(&Image) -> bool) -> Image {
        self.screen
            .capture_until(&self.output, PATIENCE, accept)
            .unwrap_or_else(|error| {
                panic!(
                    "{what}: {error}\nthe launcher logged:\n{}",
                    self.launcher.log()
                )
            })
    }

    /// What the applications started so far were run with, one list of
    /// lines each (`token=…`, then `arg=…` per argument).
    pub(crate) fn started(&self) -> Vec<Vec<String>> {
        let dir = self.dirs.path().join("started");
        fs::read_dir(dir)
            .unwrap()
            .map(|file| file.unwrap().path())
            .filter(|path| path.extension().is_none())
            .map(|path| {
                fs::read_to_string(path)
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect()
            })
            .collect()
    }
}

/// The recorder: what it was run with, to `started/<pid>` beside it,
/// written whole through a rename.
const RECORDER: &str = r#"PATH=/usr/bin:/bin
out="$(dirname "$0")/started/$$"
{
    printf 'token=%s\n' "${XDG_ACTIVATION_TOKEN-}"
    for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
} > "$out.partial" && mv "$out.partial" "$out"
"#;

/// The XDG specification's default data directories, which the session
/// sets as `XDG_DATA_DIRS`.
const SYSTEM_DATA_DIRS: [&str; 2] = ["/usr/local/share", "/usr/share"];

/// `command` with only the test's directories: its applications, its
/// configuration, and a `PATH` with nothing on it (no `xdg-terminal-exec`).
/// The system's data directories are replaced by stand-ins that hold only
/// their `vulkan` directory: the Vulkan loader finds its drivers there
/// (`vulkan/icd.d`), which the GPU renderer needs, and the launcher finds
/// none of the system's applications.
fn in_dirs(mut command: Command, dirs: &Path) -> Command {
    let none = dirs.join("none");
    let mut data_dirs = Vec::new();
    for (index, system) in SYSTEM_DATA_DIRS.iter().enumerate() {
        let stand_in = dirs.join(format!("system-data-{index}"));
        fs::create_dir_all(&stand_in).unwrap();
        let vulkan = Path::new(system).join("vulkan");
        if vulkan.is_dir() {
            std::os::unix::fs::symlink(&vulkan, stand_in.join("vulkan")).unwrap();
        }
        data_dirs.push(stand_in);
    }
    command
        .env("XDG_DATA_HOME", dirs.join("data"))
        .env("XDG_DATA_DIRS", std::env::join_paths(data_dirs).unwrap())
        .env("XDG_CONFIG_HOME", dirs.join("config"))
        .env("XDG_CONFIG_DIRS", &none)
        .env("PATH", &none);
    command
}

/// The launcher's top-left corner when it is drawn, centred: where its
/// background differs from the screen's beyond it.
pub(crate) fn at_centre(image: &Image) -> Option<(u32, u32)> {
    let x = image.width().checked_sub(WIDTH)? / 2;
    let y = image.height().checked_sub(HEIGHT)? / 2;
    let inside = image.pixel(x + 2, y + 2)?;
    let opposite = image.pixel(x + WIDTH - 3, y + HEIGHT - 3)?;
    let outside = image.pixel(x.checked_sub(2)?, y.checked_sub(2)?)?;
    (inside == opposite && inside != outside).then_some((x, y))
}

/// How many bands down the launcher's [`COLUMN`] are not its background:
/// the query field (its border and inside), then the selected result's.
/// None when the launcher is not drawn.
fn bands(image: &Image) -> usize {
    let Some((x, y)) = at_centre(image) else {
        return 0;
    };
    let background: Option<Rgba> = image.pixel(x + 2, y + 2);
    let mut bands = 0;
    let mut in_band = false;
    for row in y..y + HEIGHT {
        let off = image.pixel(x + COLUMN, row) != background;
        if off && !in_band {
            bands += 1;
        }
        in_band = off;
    }
    bands
}

/// Whether the results are drawn: a band below the query field's.
pub(crate) fn results_drawn(image: &Image) -> bool {
    bands(image) > 1
}

/// The one application started, once it has been.
pub(crate) fn the_one_started(shown: &Shown) -> Vec<String> {
    assert!(
        eventually(|| !shown.started().is_empty()),
        "nothing was started"
    );
    let mut started = shown.started();
    assert_eq!(started.len(), 1, "{started:?}");
    started.remove(0)
}

pub(crate) fn token(record: &[String]) -> &str {
    record[0].strip_prefix("token=").unwrap()
}

pub(crate) fn args(record: &[String]) -> Vec<&str> {
    record[1..]
        .iter()
        .map(|line| line.strip_prefix("arg=").unwrap())
        .collect()
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn an_empty_query_shows_nothing_and_typing_shows_the_results() {
    let mut shown = Shown::start("");
    let empty = shown.see("the launcher drawn", |image| bands(image) > 0);
    assert_eq!(bands(&empty), 1, "only the query field");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn enter_starts_the_selected_application_with_a_token_and_hides() {
    let mut shown = Shown::start("");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    let record = the_one_started(&shown);
    assert!(
        !token(&record).is_empty(),
        "no activation token: {record:?}"
    );
    assert_eq!(args(&record), ["Plain"]);
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn escape_hides_and_starts_nothing() {
    let mut shown = Shown::start("");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ESCAPE).unwrap();
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
    assert!(
        stays(|| shown.started().is_empty()),
        "{:?}",
        shown.started()
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn shown_again_it_starts_empty_with_the_keyboard_in_the_field() {
    let mut shown = Shown::start("");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ESCAPE).unwrap();
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
    assert!(client(&["show"]).success());
    // The compositor places it anew.
    shown.output = shown.find_output();
    let empty = shown.see("the launcher shown again", |image| bands(image) > 0);
    assert_eq!(bands(&empty), 1, "the last query's results are gone");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_terminal_application_starts_in_the_configured_terminal() {
    let config = "[general]\nterminal = [\"/bin/sh\", \"{recorder}\", \"terminal\"]\n";
    let mut shown = Shown::start(config);
    shown.input.type_text("terminal").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    let record = the_one_started(&shown);
    let recorder = recorder(&shown);
    assert_eq!(
        args(&record),
        ["terminal", "/bin/sh", recorder.to_str().unwrap(), "Term"]
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_terminal_application_without_a_terminal_is_reported_and_not_started() {
    let mut shown = Shown::start("");
    shown.input.type_text("terminal").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    assert!(
        eventually(|| shown.launcher.said("runs in a terminal, and there is none")),
        "the missing terminal was not reported"
    );
    assert!(
        stays(|| shown.started().is_empty()),
        "{:?}",
        shown.started()
    );
}

fn recorder(shown: &Shown) -> PathBuf {
    shown.dirs.path().join("record.sh")
}
