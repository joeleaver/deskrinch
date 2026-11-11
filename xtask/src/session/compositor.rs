//! The compositors a session can run, and how each is started headless.

use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use deskrinch_test_dbus::session;

use super::{Scale, Scaling};
use crate::error::Error;

/// The KWin socket's name; the wlroots compositors pick their own.
const KWIN_SOCKET: &str = "wayland-0";
/// Each output's size, where the compositor takes one.
const OUTPUT_SIZE: (u32, u32) = (1280, 720);

/// An XDG applications menu that lists every installed application.
///
/// KWin grants its restricted interfaces from the applications in KDE's
/// service cache, which `KService` builds from the applications menu
/// (`applications.menu`, or Plasma's `plasma-applications.menu`). Without
/// one, on a system with KWin but not Plasma's workspace (Ubuntu 26.04's
/// `kwin-wayland` on a CI runner; checked here by hiding the menu), KWin
/// 6.6 logs `"applications.menu" not found` and grants nothing. The session puts this one in its own configuration
/// directory.
const APPLICATIONS_MENU: &str = r#"<!DOCTYPE Menu PUBLIC "-//freedesktop//DTD Menu 1.0//EN"
 "http://www.freedesktop.org/standards/menu-spec/1.0/menu.dtd">
<Menu>
  <Name>Applications</Name>
  <DefaultAppDirs/>
  <DefaultDirectoryDirs/>
  <Include><All/></Include>
</Menu>
"#;

/// A compositor a session can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Compositor {
    /// KWin, `kwin_wayland --virtual`, without Xwayland:
    /// `org_kde_plasma_window_management`.
    Kwin,
    /// sway, on the wlroots headless backend:
    /// `zwlr_foreign_toplevel_management_v1`. It has no minimized state.
    Sway,
    /// labwc, on the wlroots headless backend:
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

    /// The command that starts it with `outputs` outputs side by side, in
    /// the session whose root is `root` (the caller isolates it), at `scale`
    /// where it takes a scale at start-up ([`Scaling::StartUp`]).
    pub(super) fn command(self, root: &Path, outputs: u32, scale: Scale) -> Result<Command, Error> {
        match self {
            Self::Kwin => {
                // No `--xwayland`: KWin then starts neither Xwayland nor the
                // `kcminit` that comes with it, which aborts and dumps core
                // when its X server goes away while it is still starting.
                let mut command = Command::new("kwin_wayland");
                command
                    .args(["--virtual", "--socket", KWIN_SOCKET])
                    .args([
                        "--no-lockscreen",
                        "--no-global-shortcuts",
                        "--no-kactivities",
                    ])
                    .arg(format!("--output-count={outputs}"))
                    .arg(format!("--width={}", OUTPUT_SIZE.0))
                    .arg(format!("--height={}", OUTPUT_SIZE.1));
                // Not `--scale`, which enlarges the mode instead; the session
                // scales KWin's outputs once it runs.
                write_applications_menu(root)?;
                Ok(command)
            }
            Self::Sway => {
                // No bar, background, key bindings or `exec` lines: only the
                // outputs' scale.
                let config = root.join("sway.conf");
                let contents = if scale.is_one() {
                    String::new()
                } else {
                    format!("output * scale {scale}\n")
                };
                fs::write(&config, contents).map_err(|source| Error::Io {
                    what: format!("write {}", config.display()),
                    source,
                })?;
                let mut command = Command::new("sway");
                command.arg("--config").arg(&config);
                wlroots_headless(&mut command, outputs);
                Ok(command)
            }
            Self::Labwc => {
                // An empty configuration directory: labwc's defaults, and no
                // autostart.
                let config = root.join("labwc");
                session::create_private_dir(&config).map_err(|source| Error::Io {
                    what: format!("create {}", config.display()),
                    source,
                })?;
                let mut command = Command::new("labwc");
                command
                    .arg("--config-dir")
                    .arg(&config)
                    // labwc pushes its environment into the D-Bus and systemd
                    // activation environments on a DRM backend; never here.
                    .env("LABWC_UPDATE_ACTIVATION_ENV", "false");
                wlroots_headless(&mut command, outputs);
                Ok(command)
            }
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

/// Writes [`APPLICATIONS_MENU`] as `menus/applications.menu` in the
/// session's configuration directory (`XDG_CONFIG_HOME`) under `root`.
fn write_applications_menu(root: &Path) -> Result<(), Error> {
    let menus = root.join("config").join("menus");
    let path = menus.join("applications.menu");
    fs::create_dir_all(&menus)
        .and_then(|()| fs::write(&path, APPLICATIONS_MENU))
        .map_err(|source| Error::Io {
            what: format!("write {}", path.display()),
            source,
        })
}

/// Runs a wlroots compositor on the headless backend, with software
/// rendering, no input devices and `outputs` outputs.
fn wlroots_headless(command: &mut Command, outputs: u32) {
    command
        .env("WLR_BACKENDS", "headless")
        .env("WLR_RENDERER", "pixman")
        .env("WLR_HEADLESS_OUTPUTS", outputs.to_string())
        .env("WLR_LIBINPUT_NO_DEVICES", "1");
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
    fn the_menu_goes_only_into_the_sessions_own_configuration() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        write_applications_menu(root.path()).unwrap();
        let menu = root.path().join("config/menus/applications.menu");
        assert_eq!(fs::read_to_string(&menu).unwrap(), APPLICATIONS_MENU);
        let written: Vec<_> = fs::read_dir(root.path()).unwrap().collect();
        assert_eq!(written.len(), 1, "only config/ is written");
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
