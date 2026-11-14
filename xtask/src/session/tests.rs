//! The compositor's command: what a session gives it.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use super::{Compositor, CompositorRenderer, Home, Options, Outputs, compositor_command};

/// The value `command` sets for `variable`, if it sets one.
fn set<'a>(command: &'a Command, variable: &str) -> Option<&'a OsStr> {
    command
        .get_envs()
        .find(|(name, _)| *name == OsStr::new(variable))
        .and_then(|(_, value)| value)
}

/// The compositor's command for a headless session of `compositor` under
/// `root`, with `home`.
fn command(compositor: Compositor, root: &Path, home: Home) -> Command {
    home.create_dirs(root).unwrap();
    let options = Options {
        home,
        ..Options::headless(
            compositor,
            CompositorRenderer::Software,
            Outputs::plain(1),
            None,
        )
    };
    compositor_command(&options, root, "unix:path=/nonexistent").unwrap()
}

/// Every compositor is given the session's `HOME`, under the session's
/// root: the backstop for what ignores the XDG variables, as KWin's Mesa
/// does in secure mode (see `deskrinch_test_dbus::session`).
#[test]
fn the_compositor_has_the_session_s_home() {
    for compositor in [Compositor::Kwin, Compositor::Sway, Compositor::Labwc] {
        let root = tempfile::tempdir().unwrap();
        let command = command(compositor, root.path(), Home::Fresh);
        let home = set(&command, "HOME").expect("HOME is set");
        assert!(
            Path::new(home).starts_with(root.path()),
            "{compositor:?}: HOME is {home:?}, outside {:?}",
            root.path()
        );
        assert_eq!(
            set(&command, "XDG_CACHE_HOME").map(Path::new),
            Some(root.path().join("cache").as_path()),
            "{compositor:?}"
        );
    }
}

/// The desktop's compositor, on a profile, is given a `HOME` that leads
/// into the profile.
#[test]
fn a_profile_s_compositor_has_the_profile_s_home() {
    let profile = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let home = Home::Profile(profile.path().to_owned());
    let command = command(Compositor::Kwin, root.path(), home);
    let home = Path::new(set(&command, "HOME").expect("HOME is set"));
    assert!(home.starts_with(root.path()), "{home:?}");
    assert!(
        home.canonicalize()
            .unwrap()
            .starts_with(profile.path().canonicalize().unwrap()),
        "{home:?} leads outside the profile"
    );
}
