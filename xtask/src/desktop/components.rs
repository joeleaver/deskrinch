//! The shell's components in the nested desktop: started in order, each once
//! the one before is ready, and started again, when one ends, as its
//! restart policy says (`deskrinch_shell::restart`), the way it was first
//! started.
//!
//! A component's end is decided only while the compositor runs: one that
//! ends because its compositor went away ends with the desktop, which its
//! window watch notices. Every verdict is said in one line; the limit's
//! being reached is said loudly, once, and the desktop runs on without that
//! component. A component started again that ends before it is ready has
//! failed again, and is decided again.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Instant;

use deskrinch_shell::restart::{Exit, Starts, Verdict};
use deskrinch_shell::{COMPONENTS, Component, Ready, Restart};
use deskrinch_test_component::RINCH_RENDERER_VARIABLE;
use deskrinch_test_wait::Deadline;

use super::{DesktopError, Renderer, binary};
use crate::error::Error;
use crate::session::{ClientId, POLL, Session, interrupt};

/// One component the desktop started.
#[derive(Debug)]
struct Running {
    component: &'static Component,
    id: ClientId,
    starts: Starts,
    /// Ended and not started again.
    gone: bool,
}

/// The shell's components, running in a desktop's session.
#[derive(Debug)]
pub(super) struct Components<'a> {
    binaries: &'a BTreeMap<&'static str, PathBuf>,
    renderer: Option<Renderer>,
    running: Vec<Running>,
}

impl<'a> Components<'a> {
    /// Starts the components in order, each once the one before is ready.
    pub(super) fn start(
        session: &mut Session,
        binaries: &'a BTreeMap<&'static str, PathBuf>,
        renderer: Option<Renderer>,
    ) -> Result<Self, Error> {
        let mut components = Self {
            binaries,
            renderer,
            running: Vec::new(),
        };
        for component in COMPONENTS {
            let id = components.start_one(session, component)?;
            components.running.push(Running {
                component,
                id,
                starts: Starts::new(component.restart, Instant::now()),
                gone: false,
            });
        }
        Ok(components)
    }

    /// Starts `component` and waits until it is ready.
    fn start_one(
        &self,
        session: &mut Session,
        component: &'static Component,
    ) -> Result<ClientId, Error> {
        let mut command = session.client(binary(self.binaries, component.package)?.as_os_str());
        command.args(component.args);
        if let Some(renderer) = self.renderer {
            command.env(RINCH_RENDERER_VARIABLE, renderer.rinch());
        }
        let id = session.spawn(command)?;
        if let Ready::BusName(name) = component.ready {
            wait_until_ready(session, component.package, id, name)?;
        }
        Ok(id)
    }

    /// Looks at each component still running: one that has ended is decided
    /// by its policy and said, and started again if the verdict says so,
    /// unless the compositor has gone (the desktop is ending).
    pub(super) fn look(&mut self, session: &mut Session) -> Result<(), Error> {
        for index in 0..self.running.len() {
            if self.running[index].gone {
                continue;
            }
            let Some(status) = session.client_status(self.running[index].id) else {
                continue;
            };
            interrupt::check()?;
            self.ended(session, index, status)?;
        }
        Ok(())
    }

    /// Decides component `index`, which ended with `status`, and starts it
    /// again while the verdicts say so and each start ends before it is
    /// ready.
    fn ended(
        &mut self,
        session: &mut Session,
        index: usize,
        mut status: ExitStatus,
    ) -> Result<(), Error> {
        loop {
            if !session.compositor_running() {
                return Ok(());
            }
            let running = &mut self.running[index];
            let component = running.component;
            let verdict = running.starts.after_exit(Exit::of(&status), Instant::now());
            eprintln!("{}", said(component, status, verdict));
            if !matches!(verdict, Verdict::Restart { .. }) {
                running.gone = true;
                return Ok(());
            }
            match self.start_one(session, component) {
                Ok(id) => {
                    self.running[index].id = id;
                    return Ok(());
                }
                Err(Error::Desktop(DesktopError::ComponentExited { status: again, .. })) => {
                    status = again;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// What the desktop says of `component`, which ended with `status`, given
/// the `verdict`.
fn said(component: &Component, status: ExitStatus, verdict: Verdict) -> String {
    let package = component.package;
    match verdict {
        Verdict::Restart { start } => {
            let limit = match component.restart {
                Restart::OnFailure { burst, interval } => {
                    format!(
                        "start {start} of at most {burst} in {} s",
                        interval.as_secs()
                    )
                }
                Restart::Never => format!("start {start}"),
            };
            format!("desktop: {package} failed ({status}); starting it again ({limit})")
        }
        Verdict::Clean => format!(
            "desktop: {package} exited cleanly ({status}); not started again, the desktop runs \
             on without it"
        ),
        Verdict::Never => format!(
            "desktop: {package} exited ({status}); it is never started again, the desktop runs \
             on without it"
        ),
        Verdict::Exhausted { burst, interval } => format!(
            "desktop: {package} FAILED ({status}) with {burst} starts within {} s, the most it \
             may have: NOT started again; the desktop runs on without it",
            interval.as_secs()
        ),
    }
}

/// Waits until the component `package`, client `id`, owns `name`.
fn wait_until_ready(
    session: &mut Session,
    package: &'static str,
    id: ClientId,
    name: &'static str,
) -> Result<(), Error> {
    // A component that exits fails at once: only a hung one waits it out.
    let deadline = Deadline::patience();
    loop {
        interrupt::check()?;
        if session.wait_for_name(name, POLL).is_ok() {
            return Ok(());
        }
        if let Some(status) = session.client_status(id) {
            return Err(DesktopError::ComponentExited { package, status }.into());
        }
        if deadline.passed() {
            return Err(DesktopError::ComponentNotReady { package, name }.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::time::Duration;

    use deskrinch_shell::{LAUNCHER, PANEL};

    use super::*;

    #[test]
    fn each_verdict_is_said_in_one_line() {
        // Ended by SIGKILL (9 on Linux), as `wait` reports it.
        let killed = ExitStatus::from_raw(9);
        let clean = ExitStatus::from_raw(0);
        assert_eq!(
            said(&LAUNCHER, killed, Verdict::Restart { start: 2 }),
            "desktop: deskrinch-launcher failed (signal: 9 (SIGKILL)); starting it again \
             (start 2 of at most 3 in 60 s)"
        );
        assert_eq!(
            said(&PANEL, clean, Verdict::Clean),
            "desktop: deskrinch-panel exited cleanly (exit status: 0); not started again, the \
             desktop runs on without it"
        );
        assert_eq!(
            said(
                &LAUNCHER,
                killed,
                Verdict::Exhausted {
                    burst: 3,
                    interval: Duration::from_secs(60)
                }
            ),
            "desktop: deskrinch-launcher FAILED (signal: 9 (SIGKILL)) with 3 starts within 60 s, \
             the most it may have: NOT started again; the desktop runs on without it"
        );
        assert!(!said(&PANEL, clean, Verdict::Never).contains('\n'));
    }
}
