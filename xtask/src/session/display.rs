//! How a session is started beyond its compositor and outputs: where the
//! compositor shows them ([`Display`]), whose home the session's programs
//! get ([`Home`]), and the key bindings the compositor runs commands on
//! ([`Binding`]).

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::path::{Path, PathBuf};

use deskrinch_shell::Chord;
use deskrinch_test_dbus::session::{self, SESSION_DIRS};

use super::{Compositor, CompositorRenderer, Gpu, Outputs};
use crate::error::Error;

/// Where the compositor shows its outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Display {
    /// Nowhere: on its headless (virtual) backend.
    Headless,
    /// Each output in a window of its own on another compositor, whose
    /// Wayland socket this absolute path is. Only the compositor is given
    /// it: the session's programs see the session's own compositor.
    Window(PathBuf),
}

/// The home and XDG directories the session's programs get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Home {
    /// Fresh, empty ones under the session's root, removed with it.
    Fresh,
    /// The ones in this directory, which outlive the session: the session's
    /// root links to them, and removing the root removes only the links.
    /// The runtime directory is always the session's own.
    Profile(PathBuf),
}

impl Home {
    /// Creates the session's directories under `root`: each of
    /// [`SESSION_DIRS`] private and fresh, or for a profile, the runtime
    /// directory fresh and the others links into the profile, whose own
    /// directories are made (private) where missing.
    pub(super) fn create_dirs(&self, root: &Path) -> io::Result<()> {
        let Self::Profile(profile) = self else {
            return session::create_dirs(root);
        };
        for (variable, sub) in SESSION_DIRS {
            if variable == RUNTIME {
                session::create_private_dir(&root.join(sub))?;
                continue;
            }
            let kept = profile.join(sub);
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&kept)?;
            symlink(&kept, root.join(sub))?;
        }
        Ok(())
    }
}

/// The runtime directory's variable: never a profile's.
const RUNTIME: &str = "XDG_RUNTIME_DIR";

/// A key binding the compositor is configured with: a chord, and the command
/// it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    /// A name for it, unique in the session, made of the characters of
    /// [`plain`]: KWin's binding is a file of this name.
    pub(super) id: String,
    /// The chord.
    pub(super) chord: Chord,
    /// The command line: an absolute program and its arguments, each made
    /// of the characters of [`plain`], separated by spaces.
    pub(super) command: String,
}

impl Binding {
    /// `chord` running `program` (an absolute path) with `args`, named
    /// `id`. Every word must be [`plain`], so that the command line reads
    /// the same in a shell, in labwc's XML and in a desktop entry's `Exec`,
    /// none of which then needs quoting.
    pub(crate) fn new(
        id: &str,
        chord: Chord,
        program: &Path,
        args: &[&str],
    ) -> Result<Self, Error> {
        if !program.is_absolute() {
            return Err(Error::BindingNotAbsolute(program.to_owned()));
        }
        let program = program
            .to_str()
            .ok_or_else(|| Error::BindingNeedsQuoting(program.as_os_str().to_owned()))?;
        let words = std::iter::once(id)
            .chain(std::iter::once(program))
            .chain(args.iter().copied());
        for word in words {
            if !plain(word) {
                return Err(Error::BindingNeedsQuoting(OsStr::new(word).to_owned()));
            }
        }
        let command = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Self {
            id: id.to_owned(),
            chord,
            command,
        })
    }
}

/// Whether `word` is non-empty and made only of ASCII letters, digits and
/// `/ . _ - + , : = @`, which no shell, XML attribute or desktop entry
/// `Exec` treats specially.
pub(super) fn plain(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:=@".contains(c))
}

/// Everything a session is started with.
#[derive(Debug, Clone)]
pub(crate) struct Options<'a> {
    /// The compositor.
    pub(crate) compositor: Compositor,
    /// What it renders with.
    pub(crate) renderer: CompositorRenderer,
    /// Its outputs.
    pub(crate) outputs: Outputs,
    /// `XDG_CURRENT_DESKTOP` for the compositor and the clients, if any.
    pub(crate) current_desktop: Option<&'a OsStr>,
    /// Where it shows its outputs.
    pub(crate) display: Display,
    /// The home the session's programs get.
    pub(crate) home: Home,
    /// Its key bindings.
    pub(crate) bindings: &'a [Binding],
    /// Whether the compositor gets the workspace tests' fixture: under
    /// labwc, four workspaces switched without the on-screen popup, and F12
    /// bound to the next one. Test sessions only: F12 is a key real
    /// applications use, and a binding takes it from every window. KWin's
    /// tests make their desktops themselves, and sway has no workspaces to
    /// configure, so it changes nothing there.
    pub(crate) workspace_tests_fixture: bool,
    /// Whether the session's clients may use the machine's GPU; they are
    /// told ([`Gpu`]).
    pub(crate) gpu: Gpu,
}

impl<'a> Options<'a> {
    /// A headless session of `compositor`, with a fresh home, no key
    /// bindings and the workspace tests' fixture: what the tests and
    /// `cargo xtask session` run.
    pub(crate) fn headless(
        compositor: Compositor,
        renderer: CompositorRenderer,
        outputs: Outputs,
        current_desktop: Option<&'a OsStr>,
    ) -> Self {
        Self {
            compositor,
            renderer,
            outputs,
            current_desktop,
            display: Display::Headless,
            home: Home::Fresh,
            bindings: &[],
            workspace_tests_fixture: true,
            gpu: Gpu::Machine,
        }
    }
}

#[cfg(test)]
mod tests {
    use deskrinch_shell::LAUNCHER_CHORD;

    use super::*;

    #[test]
    fn plain_words_need_no_quoting_anywhere() {
        assert!(plain("/home/me/target/release/deskrinch-launcher"));
        assert!(plain("toggle"));
        for word in [
            "", "a b", "it's", "a&b", "<x>", "\"q\"", "$HOME", "a%f", "~",
        ] {
            assert!(!plain(word), "{word:?}");
        }
    }

    #[test]
    fn a_binding_is_its_command_line() {
        let program = Path::new("/target/release/deskrinch-launcher");
        let binding = Binding::new("launcher", LAUNCHER_CHORD, program, &["toggle"]).unwrap();
        assert_eq!(binding.command, "/target/release/deskrinch-launcher toggle");
        assert!(matches!(
            Binding::new(
                "launcher",
                LAUNCHER_CHORD,
                Path::new("deskrinch-launcher"),
                &[]
            ),
            Err(Error::BindingNotAbsolute(_))
        ));
        assert!(matches!(
            Binding::new("launcher", LAUNCHER_CHORD, Path::new("/my target/x"), &[]),
            Err(Error::BindingNeedsQuoting(_))
        ));
        assert!(matches!(
            Binding::new("a b", LAUNCHER_CHORD, program, &[]),
            Err(Error::BindingNeedsQuoting(_))
        ));
    }

    #[test]
    fn a_profile_keeps_everything_but_the_runtime_directory() {
        let profile = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let home = Home::Profile(profile.path().to_owned());
        home.create_dirs(root.path()).unwrap();
        fs::write(root.path().join("home/kept"), "").unwrap();
        fs::write(root.path().join("run/gone"), "").unwrap();
        assert!(profile.path().join("home/kept").exists());
        assert!(!profile.path().join("run").exists());
        // The root's own removal leaves the profile.
        fs::remove_dir_all(root.path()).unwrap();
        assert!(profile.path().join("home/kept").exists());
        // A second session finds the profile's directories there.
        let again = tempfile::tempdir().unwrap();
        home.create_dirs(again.path()).unwrap();
        assert!(again.path().join("home/kept").exists());
    }
}
