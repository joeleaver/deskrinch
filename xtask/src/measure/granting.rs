//! What a measured program is granted under KWin: what `deskrinch-shell`
//! grants it, when it is one of the shell's components (by its file name,
//! or as `--component` names it), installed through [`crate::grant`] into
//! each session's data directory before the compositor starts; nothing
//! otherwise. A measured panel is then the panel that ships, following the
//! window list, rather than one whose taskbar gave up at its start.
//!
//! KWin grants by the executable it sees (`/proc/<pid>/exe`). Under
//! `--instructions` that is not the program but Valgrind's Callgrind tool,
//! which runs it: the grant then names that tool, found by starting it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deskrinch_shell::Component;

use super::Request;
use crate::error::Error;
use crate::session::Compositor;

/// How long the Callgrind tool has to replace Valgrind's launcher.
const TOOL_STARTS: Duration = Duration::from_secs(10);

/// A grant to install in every session of a measurement.
#[derive(Debug)]
pub(crate) struct Granting {
    component: &'static Component,
    executable: PathBuf,
}

impl Granting {
    /// What `request`'s program is granted: `None` under sway and labwc,
    /// which restrict nothing, and for a program that is no component of
    /// the shell, or one that uses nothing restricted.
    pub(crate) fn of(request: &Request) -> Result<Option<Self>, Error> {
        if request.compositor != Compositor::Kwin {
            return Ok(None);
        }
        let program = Path::new(&request.program);
        let Some(component) = crate::grant::component(program, request.component.as_deref())?
        else {
            return Ok(None);
        };
        if component.wayland_grants.is_empty() {
            return Ok(None);
        }
        let executable = if request.instructions {
            callgrind_tool()?
        } else {
            resolve(&request.program)?
        };
        Ok(Some(Self {
            component,
            executable,
        }))
    }

    /// Installs the grant into a session's data directory `data_home`.
    pub(crate) fn install(&self, data_home: &Path) -> Result<(), Error> {
        crate::grant::install(self.component, &self.executable, data_home).map(drop)
    }

    /// What a run's notes say was granted.
    pub(crate) fn note(granting: Option<&Self>) -> String {
        granting.map_or_else(
            || {
                "nothing (not one of the shell's components, uses nothing restricted, or not KWin)"
                    .to_owned()
            },
            |granting| crate::grant::describe(granting.component, &granting.executable),
        )
    }
}

/// `program`'s executable, with links resolved: a path as given, or a bare
/// name found on `PATH`.
fn resolve(program: &OsStr) -> Result<PathBuf, Error> {
    let given = Path::new(program);
    let found = if given.components().count() > 1 {
        Some(given.to_owned())
    } else {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(given))
                .find(|candidate| candidate.is_file())
        })
    };
    let found = found.ok_or_else(|| Error::Io {
        what: format!("find {} to grant it", given.display()),
        source: std::io::ErrorKind::NotFound.into(),
    })?;
    found.canonicalize().map_err(|source| Error::Io {
        what: format!("resolve {}", found.display()),
        source,
    })
}

/// The Callgrind tool's executable: Valgrind's launcher replaces itself
/// with it, so it is what `/proc/<pid>/exe` names once a run has begun. A
/// run of `sleep` under it, ended at once, shows where it is.
fn callgrind_tool() -> Result<PathBuf, Error> {
    let mut probe = Command::new("valgrind")
        .args([
            "--tool=callgrind",
            "--callgrind-out-file=/dev/null",
            "sleep",
            "60",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| Error::Spawn {
            program: "valgrind".into(),
            source,
        })?;
    let exe = Path::new("/proc").join(probe.id().to_string()).join("exe");
    let deadline = Instant::now() + TOOL_STARTS;
    let found = loop {
        let seen = std::fs::read_link(&exe).ok();
        if let Some(path) = seen.filter(|path| is_callgrind_tool(path)) {
            break Ok(path);
        }
        if Instant::now() >= deadline {
            break Err(Error::Io {
                what: "find Valgrind's Callgrind tool".into(),
                source: std::io::ErrorKind::TimedOut.into(),
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(probe.kill());
    drop(probe.wait());
    found
}

/// Whether `path` is a Callgrind tool executable (`callgrind-amd64-linux`).
fn is_callgrind_tool(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("callgrind-"))
}

#[cfg(test)]
mod tests {
    use deskrinch_shell::PANEL;

    use super::*;

    #[test]
    fn the_callgrind_tool_is_told_by_its_name() {
        assert!(is_callgrind_tool(Path::new(
            "/usr/libexec/valgrind/callgrind-amd64-linux"
        )));
        assert!(!is_callgrind_tool(Path::new("/usr/bin/valgrind")));
        assert!(!is_callgrind_tool(Path::new("/usr/bin/valgrind.bin")));
    }

    #[test]
    fn the_note_says_what_was_granted_and_to_what() {
        let granting = Granting {
            component: &PANEL,
            executable: PathBuf::from("/tmp/measure/panel-after"),
        };
        assert!(Granting::note(Some(&granting)).starts_with("org_kde_plasma_window_management"));
        assert!(Granting::note(None).starts_with("nothing"));
    }

    fn request(program: &str, component: Option<&str>, compositor: Compositor) -> Request {
        Request {
            compositor,
            runs: 1,
            renderer: None,
            expect_renderer: None,
            shader_cache: super::super::ShaderCache::Cold,
            settle: Duration::ZERO,
            idle: Duration::ZERO,
            first_frame_timeout: Duration::ZERO,
            instructions: false,
            profiles: None,
            state: None,
            program: program.into(),
            args: Vec::new(),
            summons: None,
            window: None,
            allow_errors: false,
            component: component.map(str::to_owned),
        }
    }

    #[test]
    fn a_component_is_granted_under_kwin_and_nothing_else_is() {
        let sh = resolve(OsStr::new("sh")).unwrap();
        let sh = sh.to_str().unwrap();
        let granted = Granting::of(&request(sh, Some("deskrinch-panel"), Compositor::Kwin))
            .unwrap()
            .expect("the panel is granted under KWin");
        assert_eq!(granted.component, &PANEL);
        assert_eq!(granted.executable, Path::new(sh));
        let none = |program, component, compositor| {
            Granting::of(&request(program, component, compositor))
                .unwrap()
                .is_none()
        };
        assert!(none(sh, None, Compositor::Kwin), "not a component");
        assert!(
            none(sh, Some("deskrinch-launcher"), Compositor::Kwin),
            "nothing restricted"
        );
        assert!(
            none(sh, Some("deskrinch-panel"), Compositor::Sway),
            "sway restricts nothing"
        );
        assert!(
            none(sh, Some("deskrinch-panel"), Compositor::Labwc),
            "labwc neither"
        );
    }

    #[test]
    fn a_bare_name_is_found_on_the_path() {
        let sh = resolve(OsStr::new("sh")).unwrap();
        assert!(sh.is_absolute() && sh.is_file(), "{}", sh.display());
    }
}
