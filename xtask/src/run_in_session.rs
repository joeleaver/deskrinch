//! `cargo xtask session`: runs a command as a client of a nested, headless
//! compositor in an isolated [`Session`], then ends the whole session.
//!
//! This is the one way to run something under a nested compositor (a shell
//! component, a rinch example, a screenshot tool) for deskrinch and the
//! projects it works with: the command and everything it starts get the
//! session's private directories, bus and sockets and none of the real
//! session's, and nothing of the session outlives it.

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use deskrinch_test_dbus::session;

use crate::Outcome;
use crate::error::Error;
use crate::session::{Compositor, Scale, Session, interrupt};

/// `cargo xtask session --help`: what the session is, exactly. The lists of
/// variables come from `deskrinch-test-dbus`, which sets them.
pub(crate) fn long_about() -> String {
    let dirs = item(&format!(
        "{}: private directories under a fresh /tmp/deskrinch-session-*, whose \
         path is also in DESKRINCH_SESSION_DIR.",
        session::SESSION_DIRS.map(|(name, _)| name).join(", ")
    ));
    let search = item(&format!(
        "{}: the XDG specification's defaults, not the caller's.",
        session::SEARCH_PATHS
            .map(|(name, value)| format!("{name}={value}"))
            .join(" and ")
    ));
    let removed = item(&format!(
        "None of the caller's {}.",
        session::REAL_SESSION_VARIABLES.join(", ")
    ));
    format!(
        "\
Run a command in an isolated, nested, headless compositor session.

The command runs as the only client of a fresh nested compositor (KWin, sway
or labwc, headless), and the whole session is ended when it exits. Use this,
never a hand-made session (plain dbus-run-session, a bare kwin_wayland), to run
anything under a nested compositor.

The environment the command and everything it starts see:
{dirs}
{search}
  - CARGO_HOME and RUSTUP_HOME: the caller's (or ~/.cargo and ~/.rustup of
    the real home), so cargo and rustup work with HOME moved.
  - WAYLAND_DISPLAY naming the nested compositor, and SWAYSOCK its IPC under
    sway. No X server: KWin runs without Xwayland, so no kcminit either.
  - DBUS_SESSION_BUS_ADDRESS naming a private dbus-daemon with service
    activation off: nothing (portals, secret service, notification daemons)
    is started on demand. The compositor uses the same bus.
  - XDG_CURRENT_DESKTOP only when --current-desktop sets it.
{removed}
  - Everything else as the caller has it (PATH, the locale, TERM, CARGO_* and
    RUST* settings, ...).
Caches follow XDG_CACHE_HOME and HOME, so a GPU client's shader cache (Mesa's
mesa_shader_cache, for one) is inside the session and starts empty; set
MESA_SHADER_CACHE_DIR in the command to keep one across sessions.

How it runs and ends:
  - Standard input is closed: the session's process group is not the
    terminal's, so a command that read the terminal would hang. The working
    directory is the caller's.
  - The command joins the compositor's process group. When the command
    exits, or when this task gets SIGINT, SIGTERM or SIGHUP, the group is
    sent SIGTERM, then SIGKILL after 5 s, and the bus is stopped. Then any
    process still carrying the session's XDG_RUNTIME_DIR (one that left the
    group) is killed and reported as a failure, and the session's directory
    is kept.
  - The task exits with the command's status (128 + N if signal N ended it),
    130 if interrupted, and 70 if the session could not be set up or did not
    end cleanly.

From outside this repository (a rinch clone under .work/, say), name its
xtask manifest; the command still runs in the current directory:
  cargo run -q --manifest-path /path/to/deskrinch/xtask/Cargo.toml -- \\
      session sway sh -c 'foot & sleep 2; grim shot.png'
Cargo takes its toolchain from the current directory: a rinch checkout pins
deskrinch's; elsewhere add +<channel>, the one deskrinch's
rust-toolchain.toml names.

`--keep` keeps the session's directory, with the compositor's log."
    )
}

/// `text` as an item of the help's lists: `  - ` and lines of at most 78
/// columns, continued four columns in.
fn item(text: &str) -> String {
    const WIDTH: usize = 78;
    let mut lines = vec![String::from("  -")];
    for word in text.split_whitespace() {
        let line = lines.last_mut().map_or(0, |line| line.len());
        if line + 1 + word.len() > WIDTH {
            lines.push(String::from("   "));
        }
        if let Some(line) = lines.last_mut() {
            line.push(' ');
            line.push_str(word);
        }
    }
    lines.join("\n")
}

/// The exit status when a signal ended xtask (as a shell reports SIGINT).
pub(crate) const INTERRUPTED: u8 = 130;
/// The exit status when the session could not be set up, or did not end
/// cleanly (`EX_SOFTWARE`), so that it is not mistaken for the command's.
pub(crate) const SESSION_FAILED: u8 = 70;

/// What `cargo xtask session` was asked to run.
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) compositor: Compositor,
    pub(crate) outputs: u32,
    pub(crate) scale: Scale,
    pub(crate) keep: bool,
    pub(crate) current_desktop: Option<OsString>,
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
}

/// Runs the request's command in a fresh session and returns its exit
/// status, once the session has ended.
pub(crate) fn run(request: &Request) -> Result<Outcome, Error> {
    let mut session = Session::start(
        request.compositor,
        request.outputs,
        request.scale,
        request.current_desktop.as_deref(),
        |_| Ok(()),
    )?;
    if request.keep {
        session.keep();
    }
    eprintln!(
        "session: {} with {} output(s) at scale {} in {}",
        request.compositor.name(),
        request.outputs,
        request.scale,
        session.root().display()
    );
    let status = session
        .client(&request.program)
        .args(&request.args)
        .status();
    if !session.compositor_running() {
        eprintln!(
            "session: {} exited while the command ran; its log:\n{}",
            request.compositor.name(),
            session.log()
        );
    }
    session.end()?;
    interrupt::check()?;
    let status = status.map_err(|source| Error::Spawn {
        program: request.program.to_string_lossy().into_owned(),
        source,
    })?;
    Ok(Outcome::Exit(exit_code(status)))
}

/// The exit status to pass on for a command that ended with `status`: its
/// own code, or 128 plus the signal that ended it, as a shell reports it.
fn exit_code(status: ExitStatus) -> u8 {
    let code = status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .unwrap_or(i32::from(SESSION_FAILED));
    u8::try_from(code).unwrap_or(SESSION_FAILED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_items_wrap_at_78_columns() {
        let text = "word ".repeat(40);
        let wrapped = item(&text);
        assert!(wrapped.starts_with("  - word"));
        assert!(wrapped.lines().all(|line| line.len() <= 78), "{wrapped}");
        assert!(
            wrapped
                .lines()
                .skip(1)
                .all(|line| line.starts_with("    word"))
        );
        assert_eq!(
            wrapped.split_whitespace().filter(|w| *w == "word").count(),
            40
        );
    }

    #[test]
    fn the_help_lines_fit_a_terminal() {
        let help = long_about();
        assert!(help.lines().all(|line| line.len() <= 80), "{help}");
        assert!(help.contains("XDG_VTNR"));
    }

    /// The README says what a client sees; it names every variable the
    /// session removes or sets.
    #[test]
    fn the_readme_names_every_variable() {
        let readme = include_str!("../../README.md");
        let names = session::REAL_SESSION_VARIABLES
            .iter()
            .chain(session::SESSION_DIRS.iter().map(|(name, _)| name))
            .chain(session::SEARCH_PATHS.iter().map(|(name, _)| name));
        // As code: `NAME` alone, or `NAME=value`.
        for name in names {
            assert!(
                readme.contains(&format!("`{name}`")) || readme.contains(&format!("`{name}=")),
                "README lacks `{name}`"
            );
        }
        for (name, value) in session::SEARCH_PATHS {
            assert!(readme.contains(value), "README lacks {name}={value}");
        }
    }

    #[test]
    fn passes_the_commands_code_on() {
        assert_eq!(exit_code(ExitStatus::from_raw(0)), 0);
        assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), 3);
        assert_eq!(exit_code(ExitStatus::from_raw(255 << 8)), 255);
    }

    #[test]
    fn reports_a_signal_as_a_shell_does() {
        // Raw wait statuses: the low bits are the signal that ended it.
        assert_eq!(exit_code(ExitStatus::from_raw(15)), 128 + 15);
        assert_eq!(exit_code(ExitStatus::from_raw(9)), 128 + 9);
    }
}
