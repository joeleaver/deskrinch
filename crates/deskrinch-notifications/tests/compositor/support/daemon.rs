//! The daemon and its client started: the daemon in the session with the
//! run's renderer and an icon theme of the test's own, once it owns the
//! name, and a client sending to it on the session's bus; what the daemon
//! says, and the signals the client gets.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use deskrinch_notification_service::NAME;
use deskrinch_test_client::TestClient;
use deskrinch_test_component::{Component, Renderer, programs};
use deskrinch_test_input::Input;
use deskrinch_test_notify::{Client, Signal};
use deskrinch_test_screen::{Rgba, Screen};
use deskrinch_test_wait::comes_true;
use zbus::Variant;
use zbus::fdo::DBusProxy;

use super::screen::Pictures;
use super::{Notifications, in_nested_session};

/// The colour of the test's icon for an action, `test-red`.
pub(crate) const RED_ICON: Rgba = Rgba::opaque(220, 30, 30);

/// The name of that icon in the test's theme.
pub(crate) const RED_ICON_NAME: &str = "test-red";

impl Notifications {
    /// The pointer, then the daemon with the test's icon theme, once it
    /// owns the name; nothing shown yet.
    pub(crate) fn start() -> Self {
        in_nested_session();
        let mut input = Input::connect().unwrap();
        let data = tempfile::tempdir().unwrap();
        install(data.path());
        let mut screen = Screen::connect().unwrap();
        // Out of the way of the column, so that nothing is under it.
        let first = screen.outputs()[0].clone();
        input.move_pointer(&first, 1.0, 1.0).ok();
        let before = Pictures::still(&mut screen, None);
        let mut command =
            Command::new(programs::program(deskrinch_shell::NOTIFICATIONS.package).unwrap());
        command.env("XDG_DATA_HOME", data.path());
        // A panic of the daemon's shows where, in the failure's lines.
        command.env("RUST_BACKTRACE", "1");
        // Its fit lines (`showing N, M waiting`) are at info level.
        command.env("RUST_LOG", "warn,deskrinch_notifications=info");
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
            on: None,
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

    /// The fit the daemon last said (`showing N, M waiting`): how many it
    /// shows and how many wait; `None` before it has said one.
    pub(crate) fn fit(&self) -> Option<(usize, usize)> {
        self.daemon
            .watch()
            .lines()
            .iter()
            .rev()
            .find_map(|line| fit_of(line))
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

/// The fit a daemon's line `line` says (`showing N, M waiting`), if it is
/// one.
pub(super) fn fit_of(line: &str) -> Option<(usize, usize)> {
    let (_, rest) = line.split_once("showing ")?;
    let (shown, rest) = rest.split_once(", ")?;
    let waiting = rest.trim_end().strip_suffix(" waiting")?;
    Some((shown.parse().ok()?, waiting.parse().ok()?))
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
