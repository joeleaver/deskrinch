//! Launching entries.

mod activate;
mod bus;
mod scope;
mod spawn;

use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::time::Duration;

use zbus::Connection;

use crate::entry::DesktopEntry;
use crate::env::Environment;
use crate::error::LaunchError;
use crate::exec::{self, Fields, Target};
use crate::locale::Locale;

/// How long an application has to answer D-Bus activation, which may
/// include starting it: the D-Bus default.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(25);
/// How long systemd has to answer a call.
const SYSTEMD_TIMEOUT: Duration = Duration::from_secs(5);

/// Environment variables a launcher's own activation token arrives in. They
/// are removed from what a launched program inherits, so a stale token is
/// never passed on; [`LaunchContext::with_activation_token`] sets a fresh
/// one.
const TOKEN_VARIABLES: [&str; 2] = ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"];

/// The policy and session a launch runs with.
///
/// The defaults: no terminal emulator, no activation token, and a systemd
/// scope (without a launcher name) whenever the session bus has systemd.
#[derive(Debug, Clone)]
pub struct LaunchContext {
    locales: Vec<Locale>,
    session_bus: Option<String>,
    terminal: Option<Vec<OsString>>,
    activation_token: Option<String>,
    /// Changes to the inherited environment, in the order given: a value
    /// sets the variable, `None` removes it.
    env_changes: Vec<(OsString, Option<OsString>)>,
    scope: ScopePolicy,
}

#[derive(Debug, Clone)]
enum ScopePolicy {
    Systemd { launcher: Option<String> },
    Never,
}

/// What a launch started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launched {
    /// A process, detached from the caller.
    Process {
        /// Its PID.
        pid: u32,
        /// Where it runs.
        placement: Placement,
    },
    /// A `DBusActivatable` application, asked over D-Bus.
    Activated {
        /// Its bus name.
        bus_name: String,
    },
}

/// Where a launched process runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// In a transient systemd scope of its own.
    Scope {
        /// The scope's unit name.
        unit: String,
    },
    /// Spawned directly, in the caller's cgroup.
    Direct(DirectReason),
}

/// Why a process was spawned directly rather than in a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectReason {
    /// The context turned scopes off.
    Disabled,
    /// There is no session bus, or it cannot be reached.
    NoSessionBus,
    /// No systemd user manager is on the session bus.
    NoSystemd,
    /// systemd did not create the scope; the text says why. The process
    /// runs regardless.
    ScopeFailed(String),
}

impl LaunchContext {
    /// A context for the session `env` describes: its locales (for `%c`)
    /// and its session bus.
    #[must_use]
    pub fn new(env: &Environment) -> Self {
        Self {
            locales: env.locales.clone(),
            session_bus: env.session_bus.clone(),
            terminal: None,
            activation_token: None,
            env_changes: Vec::new(),
            scope: ScopePolicy::Systemd { launcher: None },
        }
    }

    /// The terminal emulator command `Terminal=true` entries run in, such as
    /// `["foot", "--"]` or `["konsole", "-e"]`: the entry's command line is
    /// appended to it.
    #[must_use]
    pub fn with_terminal<I>(mut self, command: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<OsString>,
    {
        self.terminal = Some(command.into_iter().map(Into::into).collect());
        self
    }

    /// An activation token for the launched application, set as
    /// `XDG_ACTIVATION_TOKEN` and sent in D-Bus activation's platform data.
    #[must_use]
    pub fn with_activation_token(mut self, token: impl Into<String>) -> Self {
        self.activation_token = Some(token.into());
        self
    }

    /// An extra environment variable for launched processes.
    ///
    /// The launched environment is the caller's, without its activation
    /// token, with the context's token, and then with every
    /// [`with_env`](Self::with_env) and
    /// [`without_env`](Self::without_env) applied in the order they were
    /// given: the last one for a name wins.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env_changes.push((name.into(), Some(value.into())));
        self
    }

    /// Keeps the variable `name` out of launched processes' environment:
    /// for a launcher's own private variables, or a session's that the
    /// application must not see. Ordered with
    /// [`with_env`](Self::with_env) as it describes.
    #[must_use]
    pub fn without_env(mut self, name: impl Into<OsString>) -> Self {
        self.env_changes.push((name.into(), None));
        self
    }

    /// The launcher name in scope names, `app-<launcher>-<id>-<random>.scope`,
    /// such as `deskrinch`.
    #[must_use]
    pub fn with_scope_launcher(mut self, launcher: impl Into<String>) -> Self {
        self.scope = ScopePolicy::Systemd {
            launcher: Some(launcher.into()),
        };
        self
    }

    /// Never place processes in systemd scopes.
    #[must_use]
    pub fn without_scopes(mut self) -> Self {
        self.scope = ScopePolicy::Never;
        self
    }

    /// A connection to systemd for scopes, or why there is none.
    fn systemd(&self) -> Result<Connection, DirectReason> {
        if matches!(self.scope, ScopePolicy::Never) {
            return Err(DirectReason::Disabled);
        }
        let address = self
            .session_bus
            .as_deref()
            .ok_or(DirectReason::NoSessionBus)?;
        zbus::block_on(async {
            let connection = bus::connect(address, SYSTEMD_TIMEOUT)
                .await
                .map_err(|_| DirectReason::NoSessionBus)?;
            match bus::name_has_owner(&connection, scope::SYSTEMD).await {
                Ok(true) => Ok(connection),
                Ok(false) => Err(DirectReason::NoSystemd),
                Err(error) => Err(DirectReason::ScopeFailed(error.to_string())),
            }
        })
    }
}

/// Launches `entry`, or its action `action`, opening `targets`.
///
/// - A `DBusActivatable` entry is activated over D-Bus when the session bus
///   can be reached; otherwise its `Exec` runs, if it has one.
/// - Otherwise `Exec` is expanded by [`exec::expand`] (one process per
///   target with `%f`/`%u`), prefixed with the context's terminal when
///   `Terminal=true`, and run in `Path` with standard input from
///   `/dev/null`, inheriting the caller's environment, standard output and
///   standard error. Each process is detached from the caller and, when
///   systemd is on the session bus, placed in its own scope before it
///   execs.
///
/// # Errors
///
/// [`LaunchError`]: the entry is not an application or lacks the action,
/// its `Exec` is invalid, it needs a terminal and the context has none, a
/// process could not be started, or D-Bus activation failed. When one of
/// several processes fails to start, the ones before it keep running.
pub fn launch(
    entry: &DesktopEntry,
    action: Option<&str>,
    targets: &[Target],
    context: &LaunchContext,
) -> Result<Vec<Launched>, LaunchError> {
    let app = entry.application().ok_or(LaunchError::NotAnApplication)?;
    let action = action
        .map(|id| {
            app.actions
                .iter()
                .find(|a| a.id == id)
                .ok_or_else(|| LaunchError::UnknownAction(id.into()))
        })
        .transpose()?;
    let command = match action {
        Some(action) => action.exec.as_deref(),
        None => app.exec.as_deref(),
    };
    if app.dbus_activatable
        && let Some(launched) = activate_over_dbus(
            entry,
            action.map(|a| a.id.as_str()),
            targets,
            context,
            command.is_some(),
        )?
    {
        return Ok(vec![launched]);
    }
    let command = command.ok_or(LaunchError::NoCommand)?;
    let fields = Fields {
        name: entry.name.get(&context.locales),
        icon: entry
            .icon
            .as_ref()
            .map(|icon| icon.get(&context.locales).as_str()),
        desktop_file: Some(&entry.path),
    };
    let lines = exec::expand(command, &fields, targets)?;
    let terminal = if app.terminal {
        Some(
            context
                .terminal
                .as_deref()
                .ok_or(LaunchError::TerminalRequired)?,
        )
    } else {
        None
    };
    let systemd = context.systemd();
    let description = entry.name.get(&context.locales);
    lines
        .into_iter()
        .map(|line| {
            let line: Vec<OsString> = terminal
                .into_iter()
                .flatten()
                .cloned()
                .chain(line)
                .collect();
            let mut command = Command::new(&line[0]);
            command.args(&line[1..]).stdin(Stdio::null());
            if let Some(dir) = &app.working_dir {
                command.current_dir(dir);
            }
            for variable in TOKEN_VARIABLES {
                command.env_remove(variable);
            }
            if let Some(token) = &context.activation_token {
                command.env("XDG_ACTIVATION_TOKEN", token);
            }
            for (name, value) in &context.env_changes {
                match value {
                    Some(value) => command.env(name, value),
                    None => command.env_remove(name),
                };
            }
            let (pid, placement) = spawn::spawn_detached(&mut command, |pid| match &systemd {
                Ok(connection) => place(connection, context, entry.id.app_id(), description, pid),
                Err(reason) => Placement::Direct(reason.clone()),
            })
            .map_err(|source| LaunchError::Spawn {
                program: line[0].to_string_lossy().into_owned(),
                source,
            })?;
            Ok(Launched::Process { pid, placement })
        })
        .collect()
}

/// Moves `pid` into a new scope for `app_id`.
fn place(
    connection: &Connection,
    context: &LaunchContext,
    app_id: &str,
    description: &str,
    pid: u32,
) -> Placement {
    let launcher = match &context.scope {
        ScopePolicy::Systemd { launcher } => launcher.as_deref(),
        ScopePolicy::Never => return Placement::Direct(DirectReason::Disabled),
    };
    let unit = scope::unit_name(launcher, app_id, scope::random_part(pid));
    match zbus::block_on(scope::start_scope(connection, &unit, description, pid)) {
        Ok(()) => Placement::Scope { unit },
        Err(error) => Placement::Direct(DirectReason::ScopeFailed(error.to_string())),
    }
}

/// Activates `entry` over D-Bus. `Ok(None)` when the session bus cannot be
/// reached and `Exec` can run instead.
fn activate_over_dbus(
    entry: &DesktopEntry,
    action: Option<&str>,
    targets: &[Target],
    context: &LaunchContext,
    can_exec: bool,
) -> Result<Option<Launched>, LaunchError> {
    let bus_name = entry.id.app_id();
    if !activate::valid_bus_name(bus_name) {
        return Err(LaunchError::InvalidBusName(bus_name.into()));
    }
    let failed = |source: zbus::Error| LaunchError::Activation {
        bus_name: bus_name.into(),
        source: Box::new(source),
    };
    let Some(address) = context.session_bus.as_deref() else {
        return if can_exec {
            Ok(None)
        } else {
            Err(LaunchError::NoCommand)
        };
    };
    let request = match (action, targets.is_empty()) {
        (Some(action), _) => activate::Request::Action(action),
        (None, true) => activate::Request::Activate,
        (None, false) => activate::Request::Open(targets.iter().map(target_uri).collect()),
    };
    zbus::block_on(async {
        let connection = match bus::connect(address, ACTIVATION_TIMEOUT).await {
            Ok(connection) => connection,
            Err(_) if can_exec => return Ok(None),
            Err(error) => return Err(failed(error)),
        };
        activate::activate(
            &connection,
            bus_name,
            request,
            context.activation_token.as_deref(),
        )
        .await
        .map_err(failed)?;
        Ok(Some(Launched::Activated {
            bus_name: bus_name.into(),
        }))
    })
}

/// A target as a URI, for `Open`, which takes nothing else. A relative path
/// is made absolute against the current directory.
fn target_uri(target: &Target) -> String {
    match target {
        Target::Uri(uri) => uri.clone(),
        Target::Path(path) => {
            exec::file_uri(&std::path::absolute(path).unwrap_or_else(|_| path.clone()))
        }
    }
}
