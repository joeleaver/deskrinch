//! [`Component`]: a shell component's binary, started inside the nested
//! session and ended with the test.

use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use deskrinch_test_client::Compositor;

use crate::log;
use crate::watch::{Presented, Watch};

/// The variable `cargo xtask compositor-tests` names a run's renderer in.
pub const RENDERER_VARIABLE: &str = "DESKRINCH_TEST_RENDERER";

/// How long a component may go without presenting its first frame before
/// it is taken to be hung: two minutes.
///
/// A hang detector, not a performance expectation: it is there so that a
/// component that never presents fails instead of hanging the test, and is
/// generous enough that only a hang reaches it, a cold GPU start on a
/// loaded machine included (device and pipeline set-up alone can be silent
/// for many seconds). How fast a first frame is, is measured with
/// `cargo xtask measure`, never by a test's wait. A component that ends
/// before presenting fails at once, not after this.
pub const HUNG_AFTER: Duration = Duration::from_secs(120);

/// rinch's own variable for the renderer it presents with, which takes a
/// [`Renderer::env`] value.
pub const RINCH_RENDERER_VARIABLE: &str = "RINCH_RENDERER";

/// The renderer a component is told to present with, through rinch's
/// `RINCH_RENDERER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    /// rinch's software renderer (tiny-skia).
    Software,
    /// rinch's GPU renderer (Vello on wgpu). rinch then panics rather than
    /// fall back when the GPU will not start.
    Gpu,
}

impl Renderer {
    /// Its value for `RINCH_RENDERER`, and in [`RENDERER_VARIABLE`].
    #[must_use]
    pub fn env(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Gpu => "gpu",
        }
    }

    /// The renderer `value` names (`software` or `gpu`).
    #[must_use]
    pub fn from_env(value: &str) -> Option<Self> {
        match value {
            "software" => Some(Self::Software),
            "gpu" => Some(Self::Gpu),
            _ => None,
        }
    }

    /// The run's renderer, as `cargo xtask compositor-tests` names it in
    /// [`RENDERER_VARIABLE`]; software when the variable is unset.
    ///
    /// # Panics
    ///
    /// When the variable names no renderer: a run that asked for one this
    /// crate does not know must not quietly test another.
    #[must_use]
    pub fn of_run() -> Self {
        match std::env::var(RENDERER_VARIABLE) {
            Err(_) => Self::Software,
            Ok(value) => Self::from_env(&value)
                .unwrap_or_else(|| panic!("{RENDERER_VARIABLE}={value:?} names no renderer")),
        }
    }

    /// Whether `presented` is this renderer: for [`Gpu`](Self::Gpu), on an
    /// adapter that is a GPU, not a software Vulkan driver.
    #[must_use]
    pub fn presented(self, presented: &Presented) -> bool {
        match (self, presented) {
            (Self::Software, Presented::Software) => true,
            (Self::Gpu, Presented::Gpu(adapter)) => !adapter.is_software(),
            _ => false,
        }
    }
}

/// A component's process, its output watched; killed when dropped, and by
/// the kernel should the test process die without dropping it
/// ([`deskrinch_test_lifetime::spawn`]). The programs the component starts
/// in turn are not killed with it.
#[derive(Debug)]
pub struct Component {
    child: Child,
    watch: Watch,
}

impl Component {
    /// Starts `command` presenting with `renderer`, logging each frame
    /// (`RINCH_PERF`), which renderer presents and the adapter wgpu chose
    /// (rinch's `info` lines and wgpu's adapter line, through `RUST_LOG`,
    /// added to any `RUST_LOG` the command was given), with standard input
    /// closed and both output streams watched.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    ///
    /// # Panics
    ///
    /// Before it starts anything, unless it runs in a nested session that
    /// `cargo xtask compositor-tests` started (`Compositor::current`): run
    /// anywhere else, the component would appear on the real desktop and
    /// take its names on the real session bus.
    pub fn start(command: Command, renderer: Renderer) -> io::Result<Self> {
        let _nested = Compositor::current();
        Self::start_unguarded(command, renderer)
    }

    /// [`start`](Self::start) without the nested-session guard, for this
    /// crate's own tests, which start `sh` and `sleep`.
    fn start_unguarded(mut command: Command, renderer: Renderer) -> io::Result<Self> {
        // A filter the command was given (for lines a test waits on) is
        // kept, with what is needed added.
        let given = command
            .get_envs()
            .find(|(name, _)| *name == "RUST_LOG")
            .and_then(|(_, value)| value)
            .and_then(|value| value.to_str())
            .map(str::to_owned);
        let filter = log::rust_log(given.as_deref());
        command
            .env("RINCH_PERF", "1")
            .env(RINCH_RENDERER_VARIABLE, renderer.env())
            .env("RUST_LOG", filter)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Killed by the kernel should the test process die without
        // dropping it.
        let mut child = deskrinch_test_lifetime::spawn(command)?;
        let watch = Watch::default();
        watch.follow_child(&mut child);
        Ok(Self { child, watch })
    }

    /// What the program has written.
    #[must_use]
    pub fn watch(&self) -> &Watch {
        &self.watch
    }

    /// Its process id.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Waits up to `patience` for it to exit (a client that hands its work
    /// to another process and leaves); its status, or `None` while it still
    /// runs. What a process it started writes to the streams it inherited
    /// is still watched after it exits.
    ///
    /// # Errors
    ///
    /// When its status cannot be read.
    pub fn wait_exit(&mut self, patience: Duration) -> io::Result<Option<ExitStatus>> {
        let deadline = Instant::now() + patience;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits for its first frame, then for its saying which renderer
    /// presented it; returns that. A component that has not presented
    /// within [`HUNG_AFTER`] is taken to be hung; one that ends first fails
    /// at once.
    ///
    /// # Errors
    ///
    /// When no frame came, or it ended first; the error holds its last
    /// lines.
    pub fn wait_presented(&self) -> Result<Presented, String> {
        if self.watch.wait_first_frame(HUNG_AFTER).is_none() {
            return Err(format!(
                "no frame; its last lines:\n{}",
                self.watch.last_words()
            ));
        }
        // The two streams are read apart: the renderer's line may come after
        // the frame's.
        self.watch.wait_for_line(HUNG_AFTER, |line| {
            log::presents_with_software(line) || log::adapter(line).is_some()
        });
        self.watch.presented().ok_or_else(|| {
            format!(
                "it presented without saying with what; its last lines:\n{}",
                self.watch.tail()
            )
        })
    }

    /// Waits as [`wait_presented`](Self::wait_presented) does, and checks
    /// that `renderer` is what presented.
    ///
    /// # Errors
    ///
    /// As `wait_presented`, and when another renderer presented.
    pub fn wait_presented_with(&self, renderer: Renderer) -> Result<(), String> {
        let presented = self.wait_presented()?;
        if renderer.presented(&presented) {
            Ok(())
        } else {
            Err(format!(
                "it was to present with the {} renderer, but {presented:?} presented",
                renderer.env()
            ))
        }
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        // Already gone, or going: either way it is ended and reaped.
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Adapter;

    fn adapter(device_type: &str) -> Adapter {
        Adapter {
            name: "a".into(),
            driver: "d".into(),
            driver_info: String::new(),
            device_type: device_type.into(),
            backend: "Vulkan".into(),
        }
    }

    #[test]
    fn a_renderer_is_what_presented_and_nothing_else() {
        assert!(Renderer::Software.presented(&Presented::Software));
        assert!(!Renderer::Software.presented(&Presented::Gpu(adapter("IntegratedGpu"))));
        assert!(Renderer::Gpu.presented(&Presented::Gpu(adapter("DiscreteGpu"))));
        assert!(!Renderer::Gpu.presented(&Presented::Gpu(adapter("Cpu"))));
        assert!(!Renderer::Gpu.presented(&Presented::Software));
    }

    #[test]
    fn names_round_trip() {
        for renderer in [Renderer::Software, Renderer::Gpu] {
            assert_eq!(Renderer::from_env(renderer.env()), Some(renderer));
        }
        assert_eq!(Renderer::from_env("auto"), None);
    }

    #[test]
    fn a_started_program_is_watched_and_ended() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "echo \"[PERF] frame 1: x\" >&2; \
             echo \"INFO rinch: presenting with the software renderer (chosen)\"; \
             exec sleep 600",
        ]);
        let component = Component::start_unguarded(command, Renderer::Software).unwrap();
        let presented = component.wait_presented().unwrap();
        assert_eq!(presented, Presented::Software);
        component.wait_presented_with(Renderer::Software).unwrap();
        let wrong = component.wait_presented_with(Renderer::Gpu);
        assert!(wrong.unwrap_err().contains("gpu renderer, but Software"));
        let id = component.id();
        drop(component);
        assert!(!std::path::Path::new(&format!("/proc/{id}")).exists());
    }

    #[test]
    fn a_program_that_ends_before_presenting_fails_at_once() {
        let mut command = Command::new("sh");
        command.args(["-c", "echo 'cannot start' >&2; exit 1"]);
        let component = Component::start_unguarded(command, Renderer::Software).unwrap();
        let started = Instant::now();
        let error = component.wait_presented().unwrap_err();
        assert!(error.contains("cannot start"), "{error}");
        // Before the bound it would otherwise have waited out.
        assert!(
            started.elapsed() < HUNG_AFTER,
            "it waited {:?} for a program that had ended",
            started.elapsed()
        );
    }

    #[test]
    fn a_filter_the_command_was_given_is_kept_with_the_renderers_added() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "echo \"filter=$RUST_LOG\"; exec sleep 600"])
            .env("RUST_LOG", "warn,example=info");
        let component = Component::start_unguarded(command, Renderer::Software).unwrap();
        let expected = format!(
            "filter=warn,example=info,{},{}",
            log::RENDERER_DIRECTIVE,
            log::ADAPTER_DIRECTIVE
        );
        assert!(
            component
                .watch()
                .wait_for_line(deskrinch_test_wait::PATIENCE, |line| line == expected),
            "{}",
            component.watch().tail()
        );
    }

    #[test]
    fn waits_for_an_exit_and_still_watches_what_it_started() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            // The frame comes from what it left behind, after it exits.
            "(sleep 0.2; echo \"[PERF] frame 1: x\" >&2) & exit 3",
        ]);
        let mut client = Component::start_unguarded(command, Renderer::Software).unwrap();
        let status = client
            .wait_exit(deskrinch_test_wait::PATIENCE)
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(3));
        assert!(
            client
                .watch()
                .wait_first_frame(deskrinch_test_wait::PATIENCE)
                .is_some()
        );
    }

    #[test]
    fn refuses_to_start_anything_outside_a_nested_session() {
        if std::env::var_os("DESKRINCH_TEST_COMPOSITOR").is_some() {
            // Run inside a nested session, where starting is allowed: there
            // is nothing to refuse, and nothing is started.
            eprintln!("in a nested session: the guard lets components start here");
            return;
        }
        // The guard must stop it before anything is spawned.
        let refused = std::panic::catch_unwind(|| {
            let mut command = Command::new("sh");
            command.args(["-c", "exit 0"]);
            drop(Component::start(command, Renderer::Software));
        });
        let why = refused.expect_err("it started a component outside a nested session");
        let message = why.downcast_ref::<&str>().copied().unwrap_or_default();
        assert!(message.contains("nested compositor"), "{message:?}");
    }

    #[test]
    fn a_running_program_has_not_exited() {
        let mut command = Command::new("sleep");
        command.arg("600");
        let mut component = Component::start_unguarded(command, Renderer::Software).unwrap();
        assert!(
            component
                .wait_exit(Duration::from_millis(100))
                .unwrap()
                .is_none()
        );
    }
}
