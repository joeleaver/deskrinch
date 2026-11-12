//! Who may run `cargo xtask desktop`: the owner, from a terminal, or a test
//! inside an isolated session of ours, where the window opens in that
//! session's headless compositor.
//!
//! Anywhere else (an agent's tool call, CI, a script) the command refuses
//! before it builds or starts anything: its window would open on the real
//! desktop.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{IsTerminal, stderr, stdin};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use rustix::process::getuid;

use super::DesktopError;
use crate::session;

/// What the guard reads of the process's environment and descriptors.
#[derive(Debug, Clone, Default)]
pub(super) struct Caller<'a> {
    /// Whether standard input and standard error are both terminals.
    pub(super) interactive: bool,
    /// Whether `GITHUB_ACTIONS` is set.
    pub(super) in_ci: bool,
    /// `DESKRINCH_SESSION_DIR`.
    pub(super) session_dir: Option<&'a OsStr>,
    /// `XDG_RUNTIME_DIR`.
    pub(super) runtime_dir: Option<&'a OsStr>,
    /// `WAYLAND_DISPLAY`.
    pub(super) display: Option<&'a OsStr>,
}

/// Lets the owner, or a test in an isolated session, go on; refuses anyone
/// else with [`DesktopError::NotInteractive`].
pub(super) fn check() -> Result<(), DesktopError> {
    let session_dir = env::var_os("DESKRINCH_SESSION_DIR");
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR");
    let display = env::var_os("WAYLAND_DISPLAY");
    let caller = Caller {
        interactive: stdin().is_terminal() && stderr().is_terminal(),
        in_ci: env::var_os("GITHUB_ACTIONS").is_some(),
        session_dir: session_dir.as_deref(),
        runtime_dir: runtime_dir.as_deref(),
        display: display.as_deref(),
    };
    allowed(&caller)
}

/// Whether `caller` may run it: inside an isolated session of ours
/// ([`in_isolated_session`]), or from a terminal outside CI.
pub(super) fn allowed(caller: &Caller<'_>) -> Result<(), DesktopError> {
    if in_isolated_session(caller) || (caller.interactive && !caller.in_ci) {
        Ok(())
    } else {
        Err(DesktopError::NotInteractive)
    }
}

/// Whether the caller runs inside an isolated session of ours: its
/// `DESKRINCH_SESSION_DIR` is a session's marked root directly in `/tmp`
/// ([`session::check_root`]), and its `WAYLAND_DISPLAY` names a socket in
/// that root's runtime directory, which this user owns and which is there.
fn in_isolated_session(caller: &Caller<'_>) -> bool {
    let Some(root) = caller.session_dir.map(Path::new) else {
        return false;
    };
    if session::check_root(root).is_err() {
        return false;
    }
    let Some(socket) = socket_path(caller.display, caller.runtime_dir) else {
        return false;
    };
    let runtime = root.join("run");
    let in_runtime = socket.parent() == Some(runtime.as_path());
    in_runtime
        && fs::symlink_metadata(&socket).is_ok_and(|metadata| {
            metadata.file_type().is_socket() && metadata.uid() == getuid().as_raw()
        })
}

/// The Wayland socket `display` names: itself when absolute, else in
/// `runtime_dir`.
pub(super) fn socket_path(display: Option<&OsStr>, runtime_dir: Option<&OsStr>) -> Option<PathBuf> {
    let display = Path::new(display?);
    if display.is_absolute() {
        return Some(display.to_owned());
    }
    let runtime = Path::new(runtime_dir?);
    runtime.is_absolute().then(|| runtime.join(display))
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    /// A marked session root, as a session makes one, with a socket in its
    /// runtime directory; removed when dropped.
    struct FakeSession {
        root: tempfile::TempDir,
        _socket: UnixListener,
    }

    impl FakeSession {
        fn new() -> Self {
            let root = session::fake_root();
            fs::create_dir(root.path().join("run")).unwrap();
            let socket = UnixListener::bind(root.path().join("run/wayland-0")).unwrap();
            Self {
                root,
                _socket: socket,
            }
        }

        fn caller(&self) -> Caller<'_> {
            Caller {
                session_dir: Some(self.root.path().as_os_str()),
                runtime_dir: Some(OsStr::new("/nonexistent")),
                display: None,
                ..Caller::default()
            }
        }
    }

    #[test]
    fn the_owner_at_a_terminal_may_run_it() {
        let owner = Caller {
            interactive: true,
            ..Caller::default()
        };
        allowed(&owner).unwrap();
    }

    #[test]
    fn nobody_else_outside_a_session_may() {
        let script = Caller::default();
        assert!(matches!(
            allowed(&script),
            Err(DesktopError::NotInteractive)
        ));
        let ci = Caller {
            interactive: true,
            in_ci: true,
            ..Caller::default()
        };
        assert!(matches!(allowed(&ci), Err(DesktopError::NotInteractive)));
    }

    #[test]
    fn a_test_in_an_isolated_session_may() {
        let session = FakeSession::new();
        let socket = session.root.path().join("run/wayland-0");
        let absolute = Caller {
            display: Some(socket.as_os_str()),
            in_ci: true,
            ..session.caller()
        };
        allowed(&absolute).unwrap();
        let runtime = session.root.path().join("run");
        let relative = Caller {
            display: Some(OsStr::new("wayland-0")),
            runtime_dir: Some(runtime.as_os_str()),
            ..session.caller()
        };
        allowed(&relative).unwrap();
    }

    #[test]
    fn a_session_must_be_marked_and_its_display_its_own() {
        let session = FakeSession::new();
        // The real display, from inside a session's directory.
        let real = Caller {
            display: Some(OsStr::new("/run/user/1000/wayland-0")),
            ..session.caller()
        };
        assert!(allowed(&real).is_err());
        // A socket in the root, not its runtime directory.
        let elsewhere = session.root.path().join("wayland-0");
        let _listener = UnixListener::bind(&elsewhere).unwrap();
        let outside = Caller {
            display: Some(elsewhere.as_os_str()),
            ..session.caller()
        };
        assert!(allowed(&outside).is_err());
        // No socket there.
        let missing = session.root.path().join("run/wayland-9");
        let gone = Caller {
            display: Some(missing.as_os_str()),
            ..session.caller()
        };
        assert!(allowed(&gone).is_err());
        // Not a session's root at all.
        let plain = tempfile::tempdir_in("/tmp").unwrap();
        fs::create_dir(plain.path().join("run")).unwrap();
        let socket = plain.path().join("run/wayland-0");
        let _plain_listener = UnixListener::bind(&socket).unwrap();
        let unmarked = Caller {
            session_dir: Some(plain.path().as_os_str()),
            display: Some(socket.as_os_str()),
            ..Caller::default()
        };
        assert!(allowed(&unmarked).is_err());
    }

    #[test]
    fn a_relative_display_is_in_the_runtime_directory() {
        assert_eq!(
            socket_path(
                Some(OsStr::new("wayland-1")),
                Some(OsStr::new("/run/user/1"))
            ),
            Some(PathBuf::from("/run/user/1/wayland-1"))
        );
        assert_eq!(
            socket_path(Some(OsStr::new("/tmp/w")), None),
            Some(PathBuf::from("/tmp/w"))
        );
        assert_eq!(socket_path(Some(OsStr::new("wayland-1")), None), None);
        assert_eq!(
            socket_path(Some(OsStr::new("wayland-1")), Some(OsStr::new("run"))),
            None
        );
        assert_eq!(socket_path(None, Some(OsStr::new("/run/user/1"))), None);
    }
}
