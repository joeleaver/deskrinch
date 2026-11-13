//! `cargo xtask desktop`: the shell in a window on the real desktop.
//!
//! A nested compositor opens its outputs as windows on the compositor the
//! command runs under, and the shell's components ([`COMPONENTS`]) run in
//! it, in an isolated session ([`Session`]) like the tests': only the
//! nested compositor is given the real display, and everything inside gets
//! the session's own home, directories, bus and display. The launcher chord
//! ([`LAUNCHER_CHORD`]) is bound in the nested compositor's configuration.
//!
//! It is the owner's to run ([`guard`]): it refuses outside an interactive
//! terminal, except inside an isolated session of ours, where its window
//! opens in that session's headless compositor; that is how the tests run
//! it. It ends, and ends everything inside, when its window is closed, on
//! `SIGINT`, `SIGTERM` or `SIGHUP`, or when the command given after `--`
//! exits; should it be killed, the session's supervisor ends the session.

mod build;
mod ending;
mod guard;
mod profile;
mod window;

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use deskrinch_shell::{COMPONENTS, LAUNCHER_CHORD, LAUNCHER_TOGGLE, Ready};
use deskrinch_test_component::{RINCH_RENDERER_VARIABLE, Renderer as Rinch};

use self::ending::{Followed, follow, run_until_ended};
use self::profile::Profile;
use crate::Outcome;
use crate::error::Error;
use crate::session::{
    Binding, ClientId, Compositor, CompositorRenderer, Display, Home, Options, OutputTransform,
    Outputs, RenderNodes, Scale, Session, interrupt,
};

/// The exit status when the command refuses to run here (`EX_NOPERM`).
pub(crate) const REFUSED: u8 = 77;

/// How long a component has to be ready.
const COMPONENT_STARTUP: Duration = Duration::from_secs(60);
/// How often to look again while waiting.
pub(super) const POLL: Duration = Duration::from_millis(100);

/// `cargo xtask desktop --help`.
pub(crate) const LONG_ABOUT: &str = "\
Open the shell in a window on the real desktop, in a nested compositor.

The nested compositor (KWin by default; sway or labwc) shows each of its
outputs as a window on the desktop this runs under, and the shell's
components run inside it: the wallpaper, the launcher (hidden), then the
panel. Ctrl+Alt+Space in the window summons the launcher.

Only the nested compositor is given the real display. Everything inside runs
isolated as `cargo xtask session` describes: a fresh HOME and XDG directories
under /tmp/deskrinch-session-*, a private bus with no service activation, and
none of the real session's display or bus variables. --profile NAME keeps the
home, configuration, data, cache and state in .work/desktop/NAME/ instead, for
one desktop at a time.

The components are built first, optimised unless --debug.

It ends, with everything inside, when its windows are closed, on Ctrl+C, or
when the command after `--` exits (with that command's status).

It is for the owner to run from a terminal: it refuses (77) when standard
input or standard error is not a terminal, or in CI, except inside
`cargo xtask session`, where its window opens in that session's headless
compositor. The README lists what the nested desktop does not have.";

/// `cargo xtask desktop`'s command line.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// The compositor in the window.
    #[arg(value_enum, default_value = "kwin")]
    compositor: Compositor,
    /// Keep the home, configuration, data, cache and state in
    /// `.work/desktop/NAME/` from one run to the next, instead of fresh ones.
    /// One desktop uses a profile at a time.
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,
    /// How many outputs, each a window, 1280x720 at first.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
    outputs: u32,
    /// The outputs' scale from the start, as `cargo xtask session` takes it.
    #[arg(long, default_value = "1", value_name = "SCALE")]
    scale: Scale,
    /// The outputs' transform from the start, as `cargo xtask session`
    /// takes it.
    #[arg(long, default_value = "normal", value_name = "TRANSFORM")]
    transform: OutputTransform,
    /// `XDG_CURRENT_DESKTOP` for the compositor and everything inside
    /// (unset otherwise).
    #[arg(long, value_name = "NAME")]
    current_desktop: Option<OsString>,
    /// What the nested compositor renders with. By default, software on a
    /// machine without a GPU (KWin with `QPainter`); with one, KWin with
    /// OpenGL, and sway and labwc on the GPU when the components may
    /// present on it (as they need it to), else in software.
    #[arg(long, value_enum)]
    compositor_renderer: Option<CompositorRenderer>,
    /// What the components render with, through `RINCH_RENDERER`; rinch's
    /// own choice when not given.
    #[arg(long, value_enum)]
    renderer: Option<Renderer>,
    /// Build the components unoptimised.
    #[arg(long)]
    debug: bool,
    /// Keep the session's directory afterwards, with the compositor's log.
    #[arg(long)]
    keep: bool,
    /// A command to run inside once the shell has started; the desktop ends
    /// when it exits.
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<OsString>,
}

/// What the components render with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Renderer {
    /// rinch's GPU renderer.
    Gpu,
    /// rinch's software renderer.
    Software,
}

impl Renderer {
    /// rinch's name for it.
    fn rinch(self) -> &'static str {
        match self {
            Self::Gpu => Rinch::Gpu.env(),
            Self::Software => Rinch::Software.env(),
        }
    }
}

/// Why `cargo xtask desktop` could not run.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DesktopError {
    /// Not the owner at a terminal, and not inside an isolated session.
    #[error(
        "cargo xtask desktop opens a window on the real desktop, so only its owner runs it, from a \
         terminal: standard input and standard error must be terminals and GITHUB_ACTIONS unset. \
         Agents and tests run it inside `cargo xtask session`, where its window opens in the \
         headless session"
    )]
    NotInteractive,
    /// There is no Wayland display to open the window on.
    #[error("there is no Wayland display to open the window on: {why}")]
    NoDisplay {
        /// What is missing.
        why: String,
    },
    /// A profile's name is not one.
    #[error("a profile's name is 1 to 64 letters, digits, `-` and `_`, not {0:?}")]
    ProfileName(String),
    /// Another desktop uses the profile.
    #[error("the profile {} is in use: {why}", profile.display())]
    ProfileInUse {
        /// The profile's directory.
        profile: PathBuf,
        /// By what.
        why: &'static str,
    },
    /// `cargo` could not build the components.
    #[error("building the shell's components failed ({status})")]
    BuildFailed {
        /// How `cargo` exited.
        status: ExitStatus,
    },
    /// `cargo` built a component but did not report its binary.
    #[error("cargo reported no binary for {package}")]
    MissingBinary {
        /// The package.
        package: &'static str,
    },
    /// A component exited before it was ready.
    #[error("{package} exited ({status}) before it was ready")]
    ComponentExited {
        /// The component's package.
        package: &'static str,
        /// How it exited.
        status: ExitStatus,
    },
    /// A component was not ready in time.
    #[error("{package} did not own {name} within {COMPONENT_STARTUP:?}")]
    ComponentNotReady {
        /// The component's package.
        package: &'static str,
        /// The name it was to own.
        name: &'static str,
    },
    /// The nested compositor's outputs could not be followed.
    #[error("could not follow the nested compositor's outputs at {}", socket.display())]
    Watch {
        /// Its socket.
        socket: PathBuf,
        /// What failed.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Runs the desktop as `args` say, from the workspace at `root`, until it
/// ends.
pub(crate) fn run(root: &Path, args: &Args) -> Result<Outcome, Error> {
    guard::check()?;
    let outer = outer_display()?;
    let mut profile = args
        .profile
        .as_deref()
        .map(|name| Profile::open(root, name))
        .transpose()?;
    let binaries = build::binaries(root, args.debug)?;
    let id = [LAUNCHER_TOGGLE.package]
        .iter()
        .chain(LAUNCHER_TOGGLE.args)
        .copied()
        .collect::<Vec<_>>()
        .join("-");
    let toggle = Binding::new(
        &id,
        LAUNCHER_CHORD,
        binary(&binaries, LAUNCHER_TOGGLE.package)?,
        LAUNCHER_TOGGLE.args,
    )?;
    let bindings = [toggle];
    let nodes = RenderNodes::of_machine();
    let has_gpu = nodes.has_gpu();
    if !has_gpu && args.compositor_renderer.is_none() {
        eprintln!("desktop: no GPU found ({nodes}): the nested compositor renders in software");
    }
    let options = Options {
        compositor: args.compositor,
        renderer: compositor_renderer(
            args.compositor,
            args.compositor_renderer,
            args.renderer,
            has_gpu,
        ),
        outputs: Outputs {
            count: args.outputs,
            scale: args.scale,
            transform: args.transform,
        },
        current_desktop: args.current_desktop.as_deref(),
        display: Display::Window(outer),
        home: profile.as_ref().map_or(Home::Fresh, |profile| {
            Home::Profile(profile.dir().to_owned())
        }),
        bindings: &bindings,
        // The owner's desktop keeps F12 and labwc's own workspaces.
        workspace_tests_fixture: false,
    };
    let mut session = Session::start_with(&options, |_| Ok(()))?;
    if args.keep {
        session.keep();
    }
    if let Some(profile) = &mut profile {
        profile.claim(session.root(), session.group())?;
    }
    println!("desktop: session {}", session.root().display());
    println!("desktop: bus {}", session.bus_address());
    let components = start_components(&mut session, &binaries, args.renderer)?;
    // By now the compositor has rendered its first frames, with the
    // components in them: a KWin that could not says so in its log.
    session.check_presents()?;
    let outcome = run_started(&mut session, &components, &args.command);
    let ended = session.end();
    drop(profile);
    let outcome = outcome?;
    ended?;
    interrupt::check()?;
    Ok(outcome)
}

/// Runs the started desktop of `session`, its shell's `components` running,
/// until it ends: follows its window, then starts `command` (if any) and
/// says it is ready, so that once it has said so its window closing is
/// noticed. A compositor gone before its window is followed ends it there.
fn run_started(
    session: &mut Session,
    components: &[(&'static str, ClientId)],
    command: &[OsString],
) -> Result<Outcome, Error> {
    let window = match follow(session)? {
        Followed::Window(window) => window,
        Followed::Ended(outcome) => return Ok(outcome),
    };
    let program = match command.split_first() {
        Some((program, rest)) => {
            let mut command = session.client(program);
            command.args(rest);
            Some(session.spawn(command)?)
        }
        None => None,
    };
    println!(
        "desktop: ready; {} summons the launcher, closing the window ends the desktop",
        chord_name()
    );
    run_until_ended(session, window, components, program)
}

/// What the nested `compositor` renders with: what was `asked`, or by
/// default software on a machine without a GPU; with one, the GPU for KWin
/// (OpenGL), and for sway and labwc when the components, rendering with
/// `components`, may present on it (as rinch's GPU renderer needs).
fn compositor_renderer(
    compositor: Compositor,
    asked: Option<CompositorRenderer>,
    components: Option<Renderer>,
    has_gpu: bool,
) -> CompositorRenderer {
    match asked {
        Some(asked) => asked,
        None if !has_gpu => CompositorRenderer::Software,
        None if compositor == Compositor::Kwin => CompositorRenderer::Gpu,
        None => CompositorRenderer::for_client(compositor, components != Some(Renderer::Software)),
    }
}

/// The absolute path of the Wayland socket this runs under: the window's
/// compositor.
fn outer_display() -> Result<PathBuf, DesktopError> {
    let display = env::var_os("WAYLAND_DISPLAY");
    let runtime = env::var_os("XDG_RUNTIME_DIR");
    let socket = guard::socket_path(display.as_deref(), runtime.as_deref()).ok_or_else(|| {
        DesktopError::NoDisplay {
            why: "WAYLAND_DISPLAY is unset, or relative without an absolute XDG_RUNTIME_DIR".into(),
        }
    })?;
    if socket.exists() {
        Ok(socket)
    } else {
        Err(DesktopError::NoDisplay {
            why: format!("{} does not exist", socket.display()),
        })
    }
}

/// `package`'s binary in `binaries`.
fn binary<'a>(
    binaries: &'a BTreeMap<&'static str, PathBuf>,
    package: &'static str,
) -> Result<&'a Path, DesktopError> {
    binaries
        .get(package)
        .map(PathBuf::as_path)
        .ok_or(DesktopError::MissingBinary { package })
}

/// Starts the components in order, each once the one before is ready.
fn start_components(
    session: &mut Session,
    binaries: &BTreeMap<&'static str, PathBuf>,
    renderer: Option<Renderer>,
) -> Result<Vec<(&'static str, ClientId)>, Error> {
    let mut started = Vec::new();
    for component in COMPONENTS {
        let mut command = session.client(binary(binaries, component.package)?.as_os_str());
        command.args(component.args);
        if let Some(renderer) = renderer {
            command.env(RINCH_RENDERER_VARIABLE, renderer.rinch());
        }
        let id = session.spawn(command)?;
        if let Ready::BusName(name) = component.ready {
            wait_until_ready(session, component.package, id, name)?;
        }
        started.push((component.package, id));
    }
    Ok(started)
}

/// Waits until the component `package`, client `id`, owns `name`.
fn wait_until_ready(
    session: &mut Session,
    package: &'static str,
    id: ClientId,
    name: &'static str,
) -> Result<(), Error> {
    let deadline = Instant::now() + COMPONENT_STARTUP;
    loop {
        interrupt::check()?;
        if session.wait_for_name(name, POLL).is_ok() {
            return Ok(());
        }
        if let Some(status) = session.client_status(id) {
            return Err(DesktopError::ComponentExited { package, status }.into());
        }
        if Instant::now() >= deadline {
            return Err(DesktopError::ComponentNotReady { package, name }.into());
        }
    }
}

/// [`LAUNCHER_CHORD`] as a person reads it.
fn chord_name() -> String {
    use deskrinch_shell::{Key, Modifier};
    let modifiers = LAUNCHER_CHORD
        .modifiers
        .iter()
        .map(|modifier| match modifier {
            Modifier::Control => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Shift => "Shift",
            Modifier::Super => "Super",
        });
    let key = match LAUNCHER_CHORD.key {
        Key::Space => "Space",
    };
    modifiers.chain([key]).collect::<Vec<_>>().join("+")
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn kwin_is_the_default_and_a_command_follows_the_dashes() {
        let cli = Cli::try_parse_from(["desktop"]).unwrap();
        assert_eq!(cli.args.compositor, Compositor::Kwin);
        assert_eq!(cli.args.command.len(), 0);
        let cli = Cli::try_parse_from(["desktop", "sway", "--profile", "work", "--", "foot", "-e"])
            .unwrap();
        assert_eq!(cli.args.compositor, Compositor::Sway);
        assert_eq!(cli.args.profile.as_deref(), Some("work"));
        assert_eq!(cli.args.command, ["foot", "-e"]);
    }

    #[test]
    fn the_help_names_the_chord() {
        assert_eq!(chord_name(), "Ctrl+Alt+Space");
        assert!(LONG_ABOUT.contains(&chord_name()));
        assert!(include_str!("../../../README.md").contains(&chord_name()));
        assert!(LONG_ABOUT.lines().all(|line| line.len() <= 80));
    }

    #[test]
    fn without_a_gpu_every_nested_compositor_renders_in_software() {
        use CompositorRenderer::{Gpu, Software};
        for compositor in [Compositor::Kwin, Compositor::Sway, Compositor::Labwc] {
            assert_eq!(compositor_renderer(compositor, None, None, false), Software);
            assert_eq!(compositor_renderer(compositor, Some(Gpu), None, false), Gpu);
        }
        assert_eq!(compositor_renderer(Compositor::Kwin, None, None, true), Gpu);
        let soft = Some(Renderer::Software);
        assert_eq!(compositor_renderer(Compositor::Kwin, None, soft, true), Gpu);
        assert_eq!(compositor_renderer(Compositor::Sway, None, None, true), Gpu);
        assert_eq!(
            compositor_renderer(Compositor::Sway, None, soft, true),
            Software
        );
    }

    #[test]
    fn the_renderers_are_rinchs() {
        assert_eq!(Renderer::Gpu.rinch(), "gpu");
        assert_eq!(Renderer::Software.rinch(), "software");
    }
}
