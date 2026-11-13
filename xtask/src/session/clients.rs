//! A session's clients: the commands that run in it, and the ones it keeps
//! and reaps as it ends.

use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use deskrinch_test_dbus::session::isolate;

use super::{CURRENT_DESKTOP, Session, interrupt};
use crate::error::Error;

/// A client [`Session::spawn`] started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClientId(usize);

impl Session {
    /// Runs a test binary's ignored tests in the session, one at a time,
    /// with `filter` (libtest arguments) choosing which. Returns whether they
    /// passed.
    ///
    /// The tests are told the compositor and the session's process group, so
    /// that one can end the session underneath a connection.
    pub(crate) fn run_tests(
        &self,
        binary: &Path,
        filter: &[&str],
        env: &[(&str, &str)],
    ) -> Result<bool, Error> {
        let mut command = self.outside_client(binary.as_os_str());
        command
            .args(["--ignored", "--test-threads=1"])
            .args(filter)
            .envs(env.iter().copied())
            .env(
                "DESKRINCH_TEST_SESSION_GROUP",
                self.group.as_raw_nonzero().to_string(),
            );
        let status = command.status().map_err(|source| Error::Spawn {
            program: binary.display().to_string(),
            source,
        })?;
        interrupt::check()?;
        Ok(status.success())
    }

    /// A command that runs `program` as a client of the session: isolated
    /// from the real session; pointed at this compositor
    /// (`WAYLAND_DISPLAY`, and sway's `SWAYSOCK`) and at the session's bus
    /// (`DBUS_SESSION_BUS_ADDRESS`); told the session's directory
    /// (`DESKRINCH_SESSION_DIR`), and whether the run uses no GPU
    /// ([`Gpu`](super::Gpu)); with standard input closed; and in the
    /// session's process group, so that whatever it leaves running ends with
    /// the session.
    ///
    /// Standard input is closed because the session's group is not the
    /// terminal's foreground group: a client reading the terminal would be
    /// stopped (`SIGTTIN`) and hang the session.
    pub(crate) fn client(&self, program: &OsStr) -> Command {
        let mut command = self.outside_client(program);
        command.process_group(self.group.as_raw_nonzero().get());
        command
    }

    /// [`client`](Self::client) without joining the session's process
    /// group: for a test runner whose tests end the session by signalling
    /// that group, and must not end themselves with it. What it leaves
    /// running is still found by the check for leftovers.
    pub(super) fn outside_client(&self, program: &OsStr) -> Command {
        let mut command = Command::new(program);
        isolate(&mut command, &self.root);
        command
            .env("WAYLAND_DISPLAY", &self.socket)
            .env("DBUS_SESSION_BUS_ADDRESS", self.bus.address())
            .env("DESKRINCH_SESSION_DIR", &self.root)
            // The tests' guard against running in the real session reads it.
            .env("DESKRINCH_TEST_COMPOSITOR", self.compositor.name())
            .envs(self.compositor.client_env(&self.root.join("run")))
            .stdin(Stdio::null());
        self.gpu.tell(&mut command);
        if let Some(desktop) = &self.current_desktop {
            command.env(CURRENT_DESKTOP, desktop);
        }
        command
    }

    /// Starts `command`, a [`client`](Self::client) of the session, and
    /// keeps it: the session reaps it as it ends, so that it is not left in
    /// the session's group as a zombie. Returns which client it is.
    pub(crate) fn spawn(&mut self, mut command: Command) -> Result<ClientId, Error> {
        let program = command.get_program().to_string_lossy().into_owned();
        let child = command
            .spawn()
            .map_err(|source| Error::Spawn { program, source })?;
        self.clients.push(child);
        Ok(ClientId(self.clients.len() - 1))
    }

    /// How the client `id` ended, once it has; `None` while it runs.
    pub(crate) fn client_status(&mut self, id: ClientId) -> Option<ExitStatus> {
        self.clients
            .get_mut(id.0)
            .and_then(|child| child.try_wait().ok().flatten())
    }
}

/// Reaps whichever of `clients` have ended, so that none is left in the
/// session's group as a zombie.
pub(super) fn reap(clients: &mut [Child]) {
    for client in clients {
        drop(client.try_wait());
    }
}
