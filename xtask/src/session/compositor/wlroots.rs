//! sway and labwc: on wlroots' headless backend, or its Wayland backend in a
//! window, with the session's key bindings in their configuration.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use deskrinch_shell::{Chord, Key, Modifier};
use deskrinch_test_dbus::session;

use super::super::RenderNodes;
use super::super::display::{Binding, Display, Options};
use super::super::rendering::Wlroots;
use super::write;
use crate::error::Error;

/// The command that starts sway as `options` say, in the session under
/// `root`.
pub(super) fn sway(
    root: &Path,
    options: &Options<'_>,
    nodes: &RenderNodes,
) -> Result<Command, Error> {
    let config = root.join("sway.conf");
    write(&config, &sway_config(options))?;
    let mut command = Command::new("sway");
    command.arg("--config").arg(&config);
    backend(&mut command, options, &options.renderer.wlroots(nodes)?);
    Ok(command)
}

/// sway's configuration: the outputs' scale and transform, and no borders
/// or title bars, so that a tiled window fills the area it is given (sway
/// draws them around a tiled window even when it asks to decorate itself).
/// No bar, background or `exec` lines; no key bindings but the session's.
fn sway_config(options: &Options<'_>) -> String {
    let bindings = options
        .bindings
        .iter()
        .fold(String::new(), |mut lines, binding| {
            // Writing to a `String` cannot fail.
            let _ = writeln!(
                lines,
                "bindsym {} exec {}",
                sway_chord(binding.chord),
                binding.command
            );
            lines
        });
    format!(
        "output * scale {} transform {}\n\
         default_border none\n\
         default_floating_border none\n\
         {bindings}",
        options.outputs.scale,
        options.outputs.transform.sway_name()
    )
}

/// `chord` as sway names it: `Ctrl+Mod1+space`.
fn sway_chord(chord: Chord) -> String {
    let modifiers = chord.modifiers.iter().map(|modifier| match modifier {
        Modifier::Control => "Ctrl",
        Modifier::Alt => "Mod1",
        Modifier::Shift => "Shift",
        Modifier::Super => "Mod4",
    });
    modifiers
        .chain([keysym(chord.key)])
        .collect::<Vec<_>>()
        .join("+")
}

/// The command that starts labwc as `options` say, in the session under
/// `root`.
pub(super) fn labwc(
    root: &Path,
    options: &Options<'_>,
    nodes: &RenderNodes,
) -> Result<Command, Error> {
    // A configuration directory of the session's own: labwc's defaults, no
    // autostart, four workspaces, and the session's key bindings.
    let config = root.join("labwc");
    session::create_private_dir(&config).map_err(|source| Error::Io {
        what: format!("create {}", config.display()),
        source,
    })?;
    write(&config.join("rc.xml"), &labwc_rc(options.bindings))?;
    let mut command = Command::new("labwc");
    command
        .arg("--config-dir")
        .arg(&config)
        // labwc pushes its environment into the D-Bus and systemd activation
        // environments on a DRM backend; never here.
        .env("LABWC_UPDATE_ACTIVATION_ENV", "false");
    backend(&mut command, options, &options.renderer.wlroots(nodes)?);
    Ok(command)
}

/// labwc's `rc.xml`: four workspaces, switched without the on-screen popup
/// (which the screen tests would see), F12 to the next one (wrapping: the
/// compositor's own way to switch, which the workspace tests press through
/// `deskrinch-test-input`; a function key, since no test types one and a
/// binding takes its key from every window), and `bindings`, besides
/// labwc's own default bindings. A binding's command is
/// [plain](super::super::display::plain), so it needs no escaping in the
/// attribute.
fn labwc_rc(bindings: &[Binding]) -> String {
    let keybinds = bindings.iter().fold(String::new(), |mut lines, binding| {
        // Writing to a `String` cannot fail.
        let _ = write!(
            lines,
            "    <keybind key=\"{}\">\n      \
             <action name=\"Execute\" command=\"{}\" />\n    \
             </keybind>\n",
            labwc_chord(binding.chord),
            binding.command
        );
        lines
    });
    format!(
        "<?xml version=\"1.0\"?>\n\
         <labwc_config>\n  <desktops number=\"4\" popupTime=\"0\" />\n\
         <keyboard>\n    <default />\n    <keybind key=\"F12\">\n      \
         <action name=\"GoToDesktop\" to=\"right\" wrap=\"yes\" />\n    \
         </keybind>\n{keybinds}  </keyboard>\n</labwc_config>\n"
    )
}

/// `chord` as labwc names it: `C-A-space`.
fn labwc_chord(chord: Chord) -> String {
    let modifiers = chord.modifiers.iter().map(|modifier| match modifier {
        Modifier::Control => "C",
        Modifier::Alt => "A",
        Modifier::Shift => "S",
        Modifier::Super => "W",
    });
    modifiers
        .chain([keysym(chord.key)])
        .collect::<Vec<_>>()
        .join("-")
}

/// `key`'s XKB keysym name, as sway and labwc name it.
fn keysym(key: Key) -> &'static str {
    match key {
        Key::Space => "space",
    }
}

/// Runs a wlroots compositor on the backend `options` asks for, with its
/// outputs, rendering as `renderer` says: headless with no input devices,
/// or in a window on the compositor [`Compositor::show_on`] names, which
/// gives it that compositor's keyboard and pointer.
///
/// [`Compositor::show_on`]: super::Compositor::show_on
fn backend(command: &mut Command, options: &Options<'_>, renderer: &Wlroots) {
    let outputs = options.outputs.count.to_string();
    match options.display {
        Display::Headless => command
            .env("WLR_BACKENDS", "headless")
            .env("WLR_HEADLESS_OUTPUTS", outputs)
            .env("WLR_LIBINPUT_NO_DEVICES", "1"),
        Display::Window(_) => command
            .env("WLR_BACKENDS", "wayland")
            .env("WLR_WL_OUTPUTS", outputs),
    };
    renderer.apply(command);
}

#[cfg(test)]
mod tests {
    use deskrinch_shell::LAUNCHER_CHORD;

    use super::super::super::{CompositorRenderer, Outputs};
    use super::super::Compositor;
    use super::*;

    fn binding() -> Binding {
        let program = Path::new("/target/release/deskrinch-launcher");
        Binding::new("launcher", LAUNCHER_CHORD, program, &["toggle"]).unwrap()
    }

    fn options(bindings: &[Binding]) -> Options<'_> {
        Options {
            bindings,
            ..Options::headless(
                Compositor::Sway,
                CompositorRenderer::Software,
                Outputs::plain(2),
                None,
            )
        }
    }

    fn env(command: &Command, name: &str) -> Option<String> {
        command
            .get_envs()
            .find(|(variable, _)| *variable == name)
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned())
    }

    #[test]
    fn names_the_chord_as_each_compositor_does() {
        assert_eq!(sway_chord(LAUNCHER_CHORD), "Ctrl+Mod1+space");
        assert_eq!(labwc_chord(LAUNCHER_CHORD), "C-A-space");
        let other = Chord {
            modifiers: &[Modifier::Super, Modifier::Shift],
            key: Key::Space,
        };
        assert_eq!(sway_chord(other), "Mod4+Shift+space");
        assert_eq!(labwc_chord(other), "W-S-space");
    }

    #[test]
    fn sway_binds_only_the_sessions_bindings() {
        assert!(!sway_config(&options(&[])).contains("bindsym"));
        let bound = [binding()];
        let config = sway_config(&options(&bound));
        assert!(
            config.contains(
                "\nbindsym Ctrl+Mod1+space exec /target/release/deskrinch-launcher toggle\n"
            ),
            "{config}"
        );
    }

    #[test]
    fn labwc_has_four_workspaces_and_f12_to_switch() {
        let rc = labwc_rc(&[]);
        assert!(
            rc.contains("<desktops number=\"4\" popupTime=\"0\" />"),
            "{rc}"
        );
        assert!(rc.contains("<keybind key=\"F12\">"), "{rc}");
        assert!(
            rc.contains("<action name=\"GoToDesktop\" to=\"right\" wrap=\"yes\" />"),
            "{rc}"
        );
        assert!(rc.contains("<default />"), "{rc}");
    }

    #[test]
    fn labwc_keeps_its_own_bindings_beside_the_sessions() {
        let rc = labwc_rc(&[binding()]);
        assert!(rc.contains("<default />"), "{rc}");
        assert!(rc.contains("<keybind key=\"C-A-space\">"), "{rc}");
        assert!(
            rc.contains(
                "<action name=\"Execute\" command=\"/target/release/deskrinch-launcher toggle\" />"
            ),
            "{rc}"
        );
    }

    #[test]
    fn a_window_is_the_wayland_backend_with_one_window_per_output() {
        let mut window = Command::new("sway");
        let windowed = Options {
            display: Display::Window("/run/user/1000/wayland-0".into()),
            ..options(&[])
        };
        backend(&mut window, &windowed, &Wlroots::Pixman);
        assert_eq!(env(&window, "WLR_BACKENDS").as_deref(), Some("wayland"));
        assert_eq!(env(&window, "WLR_WL_OUTPUTS").as_deref(), Some("2"));
        assert_eq!(env(&window, "WLR_HEADLESS_OUTPUTS"), None);
        let mut headless = Command::new("sway");
        backend(&mut headless, &options(&[]), &Wlroots::Pixman);
        assert_eq!(env(&headless, "WLR_BACKENDS").as_deref(), Some("headless"));
        assert_eq!(env(&headless, "WLR_HEADLESS_OUTPUTS").as_deref(), Some("2"));
    }
}
