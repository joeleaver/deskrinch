//! [`Component`]: a shell component's binary, started inside the nested
//! session and ended with the test.

use std::io;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::log;
use crate::watch::{Presented, Watch};

/// The variable `cargo xtask compositor-tests` names a run's renderer in.
pub const RENDERER_VARIABLE: &str = "DESKRINCH_TEST_RENDERER";

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

/// A component's process, its output watched; killed when dropped.
#[derive(Debug)]
pub struct Component {
    child: Child,
    watch: Watch,
}

impl Component {
    /// Starts `command` presenting with `renderer`, logging each frame
    /// (`RINCH_PERF`), which renderer presents and the adapter wgpu chose
    /// (rinch's `info` lines and wgpu's adapter line, through `RUST_LOG`),
    /// with standard input closed and both output streams watched.
    ///
    /// # Errors
    ///
    /// When the program cannot be started.
    pub fn start(mut command: Command, renderer: Renderer) -> io::Result<Self> {
        let filter = format!("warn,rinch=info,{}", log::ADAPTER_DIRECTIVE);
        let mut child = command
            .env("RINCH_PERF", "1")
            .env("RINCH_RENDERER", renderer.env())
            .env("RUST_LOG", filter)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
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

    /// Waits up to `patience` for its first frame, then for its saying
    /// which renderer presented it; returns that.
    ///
    /// # Errors
    ///
    /// When no frame came, or it ended first; the error holds its last
    /// lines.
    pub fn wait_presented(&self, patience: Duration) -> Result<Presented, String> {
        if self.watch.wait_first_frame(patience).is_none() {
            return Err(format!(
                "no frame; its last lines:\n{}",
                self.watch.last_words()
            ));
        }
        // The two streams are read apart: the renderer's line may come after
        // the frame's.
        self.watch.wait_for_line(patience, |line| {
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
    pub fn wait_presented_with(
        &self,
        renderer: Renderer,
        patience: Duration,
    ) -> Result<(), String> {
        let presented = self.wait_presented(patience)?;
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
        let component = Component::start(command, Renderer::Software).unwrap();
        let presented = component.wait_presented(Duration::from_secs(600)).unwrap();
        assert_eq!(presented, Presented::Software);
        let patience = Duration::from_secs(600);
        component
            .wait_presented_with(Renderer::Software, patience)
            .unwrap();
        let wrong = component.wait_presented_with(Renderer::Gpu, patience);
        assert!(wrong.unwrap_err().contains("gpu renderer, but Software"));
        let id = component.id();
        drop(component);
        assert!(!std::path::Path::new(&format!("/proc/{id}")).exists());
    }

    #[test]
    fn waits_for_an_exit_and_still_watches_what_it_started() {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            // The frame comes from what it left behind, after it exits.
            "(sleep 0.2; echo \"[PERF] frame 1: x\" >&2) & exit 3",
        ]);
        let mut client = Component::start(command, Renderer::Software).unwrap();
        let status = client.wait_exit(Duration::from_secs(600)).unwrap().unwrap();
        assert_eq!(status.code(), Some(3));
        assert!(
            client
                .watch()
                .wait_first_frame(Duration::from_secs(600))
                .is_some()
        );
    }

    #[test]
    fn a_running_program_has_not_exited() {
        let mut command = Command::new("sleep");
        command.arg("600");
        let mut component = Component::start(command, Renderer::Software).unwrap();
        assert!(
            component
                .wait_exit(Duration::from_millis(100))
                .unwrap()
                .is_none()
        );
    }
}
