//! The compositors a session can run, and how each is started: headless, or
//! in a window on another compositor ([`Display`]), with the session's key
//! bindings in its configuration.

mod kwin;
mod wlroots;

use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::display::{Display, Options};
use super::{RenderNodes, Scaling};
use crate::error::Error;

/// Each output's size, where the compositor takes one: the mode of a
/// headless output, the first size of a window.
const OUTPUT_SIZE: (u32, u32) = (1280, 720);

/// A compositor a session can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Compositor {
    /// KWin, `kwin_wayland --virtual` (or in a window), without Xwayland:
    /// `org_kde_plasma_window_management`.
    Kwin,
    /// sway, on the wlroots headless (or Wayland) backend:
    /// `zwlr_foreign_toplevel_management_v1`. It has no minimized state.
    Sway,
    /// labwc, on the wlroots headless (or Wayland) backend:
    /// `zwlr_foreign_toplevel_management_v1`, with minimize.
    Labwc,
}

impl Compositor {
    /// Its name, as the tests read it from `DESKRINCH_TEST_COMPOSITOR`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Kwin => "kwin",
            Self::Sway => "sway",
            Self::Labwc => "labwc",
        }
    }

    /// The command that starts it as `options` say, in the session whose
    /// root is `root` (the caller isolates it, then calls
    /// [`show_on`](Self::show_on)): its outputs side by side, at their
    /// scale and transform where it takes them at start-up
    /// ([`Scaling::StartUp`]), and the key bindings in its configuration.
    pub(super) fn command(self, root: &Path, options: &Options<'_>) -> Result<Command, Error> {
        let nodes = RenderNodes::of_machine();
        match self {
            Self::Kwin => kwin::command(root, options, &nodes),
            Self::Sway => wlroots::sway(root, options, &nodes),
            Self::Labwc => wlroots::labwc(root, options, &nodes),
        }
    }

    /// Points its `command`, once isolated, at the compositor whose window
    /// it shows its outputs in, if any: KWin by its argument, sway and labwc
    /// through `WAYLAND_DISPLAY`, which they replace with their own socket
    /// for what they start.
    pub(super) fn show_on(self, command: &mut Command, display: &Display) {
        let Display::Window(outer) = display else {
            return;
        };
        match self {
            Self::Kwin => {
                command.arg("--wayland-display").arg(outer);
            }
            Self::Sway | Self::Labwc => {
                command.env("WAYLAND_DISPLAY", outer);
            }
        }
    }

    /// Whether the session must check that it composites with OpenGL:
    /// KWin falls back to `QPainter` without a DRM device, and then takes no
    /// screenshots (see [`super::compositing`]). Only headless: a window's
    /// KWin is for a person to use, which `QPainter` serves.
    pub(super) fn checks_opengl(self, display: &Display) -> bool {
        self == Self::Kwin && *display == Display::Headless
    }

    /// Why the compositor may not have taken bus name `name`.
    pub(super) fn missing_name_hint(self, name: &str) -> &'static str {
        match (self, name) {
            (Self::Kwin, "org.kde.KWin.ScreenShot2") => {
                "KWin registers it from its screenshot plugin, which Debian and Ubuntu ship in \
                 kwin-common (only recommended by kwin-wayland)"
            }
            _ => "the compositor registers it while it starts",
        }
    }

    /// How its outputs get the session's scale.
    pub(super) fn scaling(self) -> Scaling {
        match self {
            Self::Kwin => Scaling::KscreenDoctor,
            Self::Sway => Scaling::StartUp,
            Self::Labwc => Scaling::OutputManagement,
        }
    }

    /// The D-Bus names it owns on the session's bus once it has started,
    /// in the order it takes them: a session is ready for its clients only
    /// when it owns them all. KWin registers its services on the bus after
    /// it opens its Wayland socket, the screenshot interface last among
    /// those clients use; the wlroots compositors own none.
    pub(super) fn bus_names(self) -> &'static [&'static str] {
        match self {
            Self::Kwin => &["org.kde.KWin", "org.kde.KWin.ScreenShot2"],
            Self::Sway | Self::Labwc => &[],
        }
    }

    /// Variables a client needs to reach this compositor beyond
    /// `WAYLAND_DISPLAY`, given the session's runtime directory.
    pub(super) fn client_env(self, runtime: &Path) -> Vec<(&'static str, PathBuf)> {
        match self {
            Self::Sway => sway_ipc(runtime)
                .map(|ipc| ("SWAYSOCK", ipc))
                .into_iter()
                .collect(),
            Self::Kwin | Self::Labwc => Vec::new(),
        }
    }
}

/// Writes `contents` to `path`.
fn write(path: &Path, contents: &str) -> Result<(), Error> {
    fs::write(path, contents).map_err(|source| Error::Io {
        what: format!("write {}", path.display()),
        source,
    })
}

/// The name of the Wayland socket in `runtime`, once there is one.
pub(super) fn wayland_socket(runtime: &Path) -> Option<String> {
    sockets(runtime).find(|name| is_wayland_socket_name(name))
}

/// sway's IPC socket in `runtime`.
fn sway_ipc(runtime: &Path) -> Option<PathBuf> {
    sockets(runtime)
        .find(|name| is_sway_ipc_name(name))
        .map(|name| runtime.join(name))
}

/// The names of the sockets in `dir`.
fn sockets(dir: &Path) -> impl Iterator<Item = String> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_socket()))
        .filter_map(|entry| entry.file_name().into_string().ok())
}

/// Whether `name` is a compositor's `wayland-*` socket (not its lock file,
/// and not another program's socket).
fn is_wayland_socket_name(name: &str) -> bool {
    name.strip_prefix("wayland-")
        .is_some_and(|rest| !rest.is_empty() && !rest.contains('.'))
}

/// Whether `name` is sway's IPC socket, `sway-ipc.<uid>.<pid>.sock`.
fn is_sway_ipc_name(name: &str) -> bool {
    name.strip_prefix("sway-ipc.")
        .and_then(|rest| rest.strip_suffix(".sock"))
        .is_some_and(|middle| !middle.is_empty())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    #[test]
    fn a_missing_screenshot_name_points_at_the_plugin() {
        let hint = Compositor::Kwin.missing_name_hint("org.kde.KWin.ScreenShot2");
        assert!(hint.contains("kwin-common"), "{hint}");
    }

    #[test]
    fn only_a_headless_kwin_must_composite_with_opengl() {
        let window = Display::Window("/run/user/1000/wayland-0".into());
        assert!(Compositor::Kwin.checks_opengl(&Display::Headless));
        assert!(!Compositor::Kwin.checks_opengl(&window));
        assert!(!Compositor::Sway.checks_opengl(&Display::Headless));
    }

    #[test]
    fn only_the_compositor_is_shown_the_outer_display() {
        let outer = Path::new("/run/user/1000/wayland-0");
        let window = Display::Window(outer.to_owned());
        let mut kwin = Command::new("kwin_wayland");
        Compositor::Kwin.show_on(&mut kwin, &window);
        let args: Vec<_> = kwin.get_args().collect();
        assert_eq!(args, ["--wayland-display", "/run/user/1000/wayland-0"]);
        assert_eq!(kwin.get_envs().count(), 0);
        let mut sway = Command::new("sway");
        Compositor::Sway.show_on(&mut sway, &window);
        let envs: Vec<_> = sway.get_envs().collect();
        assert_eq!(
            envs,
            [("WAYLAND_DISPLAY".as_ref(), Some(outer.as_os_str()))]
        );
        let mut headless = Command::new("labwc");
        Compositor::Labwc.show_on(&mut headless, &Display::Headless);
        assert_eq!(headless.get_envs().count(), 0);
    }

    #[test]
    fn recognises_sways_ipc_socket() {
        assert!(is_sway_ipc_name("sway-ipc.1000.4242.sock"));
        assert!(!is_sway_ipc_name("wayland-1"));
        assert!(!is_sway_ipc_name("sway-ipc.1000.4242.sock.lock"));
    }

    #[test]
    fn recognises_wayland_sockets() {
        assert!(is_wayland_socket_name("wayland-0"));
        assert!(is_wayland_socket_name("wayland-1"));
        assert!(!is_wayland_socket_name("wayland-0.lock"));
        assert!(!is_wayland_socket_name("wayland-"));
        assert!(!is_wayland_socket_name("sway-ipc.1000.1.sock"));
    }

    #[test]
    fn finds_only_sockets() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        fs::write(dir.path().join("wayland-0"), "").unwrap();
        fs::write(dir.path().join("sway-ipc.1000.1.sock"), "").unwrap();
        assert_eq!(wayland_socket(dir.path()), None, "a plain file is not one");
        assert_eq!(sway_ipc(dir.path()), None);
        fs::remove_file(dir.path().join("wayland-0")).unwrap();
        fs::remove_file(dir.path().join("sway-ipc.1000.1.sock")).unwrap();
        let _wayland = UnixListener::bind(dir.path().join("wayland-0")).unwrap();
        let _ipc = UnixListener::bind(dir.path().join("sway-ipc.1000.1.sock")).unwrap();
        assert_eq!(wayland_socket(dir.path()).as_deref(), Some("wayland-0"));
        assert_eq!(
            Compositor::Sway.client_env(dir.path()),
            [("SWAYSOCK", dir.path().join("sway-ipc.1000.1.sock"))]
        );
        assert_eq!(Compositor::Labwc.client_env(dir.path()), []);
    }
}
