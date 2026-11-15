//! KWin: headless on its virtual backend, or in a window on its Wayland
//! backend, with the session's key bindings as global shortcuts.
//!
//! KWin 6 runs the global-shortcuts daemon (kglobalacceld) inside itself:
//! with global shortcuts on it takes `org.kde.kglobalaccel` on the session's
//! bus and needs nothing started there. It finds shortcuts in the
//! applications KDE's service cache lists: a binding is an application of
//! the session's own (`$XDG_DATA_HOME/applications/`, hidden from menus),
//! whose `X-KDE-Shortcuts` is the default shortcut of its `_launch` action,
//! which runs its `Exec`. A shortcut changed in the session
//! (`kglobalshortcutsrc`) overrides that default and, in a profile, stays.
//! (A desktop file in `$XDG_DATA_HOME/kglobalaccel/` is not read: KWin 6.6
//! registered no component for one.)

use std::fs;
use std::path::Path;
use std::process::Command;

use deskrinch_shell::{Chord, Key, Modifier};

use super::super::display::{Binding, Display, Options};
use super::super::{CompositorRenderer, RenderNodes};
use super::{OUTPUT_SIZE, write};
use crate::error::Error;

/// The KWin socket's name; the wlroots compositors pick their own.
const KWIN_SOCKET: &str = "wayland-0";

/// An XDG applications menu that lists every installed application.
///
/// KWin grants its restricted interfaces from the applications in KDE's
/// service cache, which `KService` builds from the applications menu
/// (`applications.menu`, or Plasma's `plasma-applications.menu`). Without
/// one, on a system with KWin but not Plasma's workspace (Ubuntu 26.04's
/// `kwin-wayland` on a CI runner; checked here by hiding the menu), KWin
/// 6.6 logs `"applications.menu" not found` and grants nothing. The session
/// puts this one in its own configuration directory.
const APPLICATIONS_MENU: &str = r#"<!DOCTYPE Menu PUBLIC "-//freedesktop//DTD Menu 1.0//EN"
 "http://www.freedesktop.org/standards/menu-spec/1.0/menu.dtd">
<Menu>
  <Name>Applications</Name>
  <DefaultAppDirs/>
  <DefaultDirectoryDirs/>
  <Include><All/></Include>
</Menu>
"#;

/// The command that starts KWin as `options` say, in the session under
/// `root`.
pub(super) fn command(
    root: &Path,
    options: &Options<'_>,
    nodes: &RenderNodes,
) -> Result<Command, Error> {
    // No `--xwayland`: KWin then starts neither Xwayland nor the `kcminit`
    // that comes with it, which aborts and dumps core when its X server goes
    // away while it is still starting.
    //
    // Not `--no-kactivities`, which must not come back: KWin 6.6 started
    // with it crashes (SIGSEGV, in the effect's QML) as soon as its
    // Overview or grid opens, which its top-left corner does when the
    // pointer is pushed into it. Without it KWin finds no activity manager
    // on the session's bus (none is activated there) and does without, at
    // no cost to its start-up, and its effects work.
    let mut command = Command::new("kwin_wayland");
    command
        .args(["--socket", KWIN_SOCKET])
        .arg("--no-lockscreen")
        .arg(format!("--output-count={}", options.outputs.count))
        .arg(format!("--width={}", OUTPUT_SIZE.0))
        .arg(format!("--height={}", OUTPUT_SIZE.1));
    // In a window, the Wayland backend: `show_on` names the compositor.
    // There, rendering in software is compositing with QPainter; headless,
    // KWin composites with OpenGL either way (its screenshots need it).
    if options.display == Display::Headless {
        command.arg("--virtual");
    } else if options.renderer == CompositorRenderer::Software {
        command.env("KWIN_COMPOSE", "Q");
    }
    // Not `--scale`, which enlarges the mode instead; the session scales
    // KWin's outputs once it runs.
    if options.bindings.is_empty() {
        command.arg("--no-global-shortcuts");
    } else {
        write_shortcuts(root, options.bindings)?;
    }
    write_applications_menu(root)?;
    // A GPU must be there when one is asked for.
    if options.renderer == CompositorRenderer::Gpu && !nodes.has_gpu() {
        return Err(Error::NoGpu {
            nodes: nodes.to_string(),
        });
    }
    // KWin's virtual backend takes `CI` to mean KWin's own CI and then
    // opens only `/dev/dri/card1`; on any other machine (GitHub's runners
    // set `CI` too) it must look for a DRM device itself.
    command.env_remove("CI");
    Ok(command)
}

/// Writes [`APPLICATIONS_MENU`] as `menus/applications.menu` in the
/// session's configuration directory (`XDG_CONFIG_HOME`) under `root`.
fn write_applications_menu(root: &Path) -> Result<(), Error> {
    let menus = root.join("config").join("menus");
    fs::create_dir_all(&menus).map_err(|source| Error::Io {
        what: format!("create {}", menus.display()),
        source,
    })?;
    write(&menus.join("applications.menu"), APPLICATIONS_MENU)
}

/// Writes each binding as an application, hidden from menus, in the
/// session's application directory (`XDG_DATA_HOME/applications`) under
/// `root`.
fn write_shortcuts(root: &Path, bindings: &[Binding]) -> Result<(), Error> {
    let dir = root.join("data").join("applications");
    fs::create_dir_all(&dir).map_err(|source| Error::Io {
        what: format!("create {}", dir.display()),
        source,
    })?;
    for binding in bindings {
        write(
            &dir.join(format!("{}.desktop", binding.id)),
            &shortcut_entry(binding),
        )?;
    }
    Ok(())
}

/// The desktop file of `binding`: its command, and its chord as the default
/// shortcut.
fn shortcut_entry(binding: &Binding) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={id}\n\
         Exec={command}\n\
         NoDisplay=true\n\
         X-KDE-Shortcuts={chord}\n",
        id = binding.id,
        command = binding.command,
        chord = shortcut(binding.chord),
    )
}

/// `chord` as Qt names a key sequence: `Ctrl+Alt+Space`.
fn shortcut(chord: Chord) -> String {
    let modifiers = chord.modifiers.iter().map(|modifier| match modifier {
        Modifier::Control => "Ctrl",
        Modifier::Alt => "Alt",
        Modifier::Shift => "Shift",
        Modifier::Super => "Meta",
    });
    let key = match chord.key {
        Key::Space => "Space",
    };
    modifiers.chain([key]).collect::<Vec<_>>().join("+")
}

#[cfg(test)]
mod tests {
    use deskrinch_shell::LAUNCHER_CHORD;

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
    fn in_a_window_software_is_qpainter_and_headless_is_opengl() {
        use super::super::super::{Compositor, Outputs};

        let compose = |display: Display| {
            let root = tempfile::tempdir_in("/tmp").unwrap();
            let options = Options {
                display,
                ..Options::headless(
                    Compositor::Kwin,
                    CompositorRenderer::Software,
                    Outputs::plain(1),
                    None,
                )
            };
            let command = command(root.path(), &options, &RenderNodes::of_machine()).unwrap();
            command
                .get_envs()
                .find(|(name, _)| *name == "KWIN_COMPOSE")
                .and_then(|(_, value)| value.map(ToOwned::to_owned))
        };
        let window = Display::Window("/run/user/1000/wayland-0".into());
        assert_eq!(compose(window), Some("Q".into()));
        assert_eq!(compose(Display::Headless), None);
    }

    #[test]
    fn activities_are_never_turned_off() {
        use super::super::super::{Compositor, Outputs};

        for display in [
            Display::Headless,
            Display::Window("/run/user/1000/wayland-0".into()),
        ] {
            let root = tempfile::tempdir_in("/tmp").unwrap();
            let options = Options {
                display,
                ..Options::headless(
                    Compositor::Kwin,
                    CompositorRenderer::Software,
                    Outputs::plain(1),
                    None,
                )
            };
            let command = command(root.path(), &options, &RenderNodes::of_machine()).unwrap();
            assert!(
                command.get_args().all(|arg| arg != "--no-kactivities"),
                "{command:?}"
            );
        }
    }

    #[test]
    fn names_the_chord_as_qt_does() {
        assert_eq!(shortcut(LAUNCHER_CHORD), "Ctrl+Alt+Space");
        let every = Chord {
            modifiers: &[Modifier::Super, Modifier::Shift],
            key: Key::Space,
        };
        assert_eq!(shortcut(every), "Meta+Shift+Space");
    }

    #[test]
    fn a_binding_is_a_hidden_application_with_a_default_shortcut() {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let program = Path::new("/target/release/deskrinch-launcher");
        let binding = Binding::new("launcher", LAUNCHER_CHORD, program, &["toggle"]).unwrap();
        write_shortcuts(root.path(), &[binding]).unwrap();
        let path = root.path().join("data/applications/launcher.desktop");
        let entry = fs::read_to_string(&path).unwrap();
        assert!(
            entry.contains("\nExec=/target/release/deskrinch-launcher toggle\n"),
            "{entry}"
        );
        assert!(
            entry.contains("\nX-KDE-Shortcuts=Ctrl+Alt+Space\n"),
            "{entry}"
        );
        assert!(entry.contains("\nNoDisplay=true\n"), "{entry}");
    }
}
