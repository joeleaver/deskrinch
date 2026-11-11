//! `deskrinch-launcher`: the application launcher, summoned over the
//! session bus.
//!
//! - `deskrinch-launcher [--show]` runs the launcher, hidden (or shown),
//!   for the rest of the session. A second one finds the first and leaves.
//! - `deskrinch-launcher toggle | show | hide` asks the running launcher, as
//!   a key binding does. With none running, `toggle` and `show` start one,
//!   shown.
//!
//! The window is a `wlr-layer-shell` surface (see [`surface`]): centred, in
//! the overlay layer, with all the keyboard while it is shown.
//!
//! # Look, logging, rendering
//!
//! As the panel's: the shell's one look (`deskrinch-theme`), following the
//! desktop's colour scheme; `tracing` on standard error, `RUST_LOG` picking
//! what is shown; both of rinch's renderers, chosen at run time
//! (`RINCH_RENDERER` overrides). Built with the `debug` feature it runs
//! rinch's debug server, for the rinch MCP tools.

mod apps;
mod icons;
mod root;
mod start;
mod surface;
mod view;

use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use deskrinch_bus::Bus;
use deskrinch_component::{Configuration, report};
use deskrinch_desktop_entries::Environment;
use deskrinch_icons::SearchPath;
use deskrinch_launcher::{Answer, Request, ServeError, Service, Visibility, send};
use deskrinch_settings::general::GeneralSettings;
use deskrinch_theme::Theme;
use deskrinch_xdg::BaseDirs;
use rinch::prelude::*;

/// The launcher, or a request to the running one.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Run the launcher shown, rather than hidden.
    #[arg(long)]
    show: bool,
    #[command(subcommand)]
    request: Option<Ask>,
}

/// A request to the running launcher.
#[derive(Debug, Clone, Copy, Subcommand)]
enum Ask {
    /// Show it if hidden, hide it if shown; start it, shown, if none runs.
    Toggle,
    /// Show it; start it, shown, if none runs.
    Show,
    /// Hide it; nothing if none runs.
    Hide,
}

impl From<Ask> for Request {
    fn from(ask: Ask) -> Self {
        match ask {
            Ask::Toggle => Self::Toggle,
            Ask::Show => Self::Show,
            Ask::Hide => Self::Hide,
        }
    }
}

fn main() -> ExitCode {
    // Before anything is reported.
    report::init();
    let cli = Cli::parse();
    match cli.request {
        Some(ask) => request(ask),
        None => run(cli.show),
    }
}

/// Asks the running launcher; starts one, shown, when a toggle or a show
/// finds none.
fn request(ask: Ask) -> ExitCode {
    match send(&Bus::Session, ask.into()) {
        Ok(Answer::Done { .. }) => ExitCode::SUCCESS,
        Ok(Answer::NotRunning) if matches!(ask, Ask::Hide) => ExitCode::SUCCESS,
        Ok(Answer::NotRunning) => start_shown(),
        Err(error) => {
            report::error("asking the launcher", &error);
            ExitCode::FAILURE
        }
    }
}

/// Starts a launcher, shown, in a process group of its own, so that it
/// outlives the command that started it (a key binding's).
fn start_shown() -> ExitCode {
    use std::os::unix::process::CommandExt as _;
    let started = std::env::current_exe().and_then(|exe| {
        Command::new(exe)
            .arg("--show")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .process_group(0)
            .spawn()
    });
    match started {
        Ok(_launcher) => ExitCode::SUCCESS,
        Err(error) => {
            report::error("starting the launcher", &error);
            ExitCode::FAILURE
        }
    }
}

/// Runs the launcher until the session ends.
fn run(show: bool) -> ExitCode {
    let visibility = Arc::new(Visibility::new(show));
    let service = match Service::start(&Bus::Session, Arc::clone(&visibility)) {
        Ok(service) => service,
        Err(ServeError::Running) => {
            tracing::info!("a launcher is already running; leaving it to answer");
            return if show {
                request(Ask::Show)
            } else {
                ExitCode::SUCCESS
            };
        }
        Err(error) => {
            report::error("taking the launcher's place on the session bus", &error);
            return ExitCode::FAILURE;
        }
    };
    let base_dirs = BaseDirs::from_process();
    let configuration = Configuration::start(&base_dirs);
    let theme = Theme::start(&configuration, Bus::Session);
    // For the terminal applications run in; the look follows `[general]`
    // through the theme.
    let general = configuration.follow::<GeneralSettings>();
    let first_theme = theme.initial();
    let env = Environment::from_process();
    let apps = apps::Apps::start(&env);
    let search_path = SearchPath::new(&base_dirs);
    let icons = icons::Icons::start(search_path.clone());
    let parts = root::Parts {
        theme,
        general,
        visibility,
        made_shown: show,
        apps,
        icons,
        search_path,
        env,
    };
    App::new(move |scope| root::launcher(scope, parts))
        .window_props(surface::window_props(show))
        .theme(first_theme)
        .run();
    // The name is the session's for as long as the launcher runs.
    drop(service);
    ExitCode::SUCCESS
}
