//! What a measured program is granted under KWin: what `deskrinch-shell`
//! grants it, when it is one of the shell's components (by its file name,
//! or as `--component` names it), installed through [`crate::grant`] into
//! each session's data directory before the compositor starts; nothing
//! otherwise, for a reason each run's notes give. A measured panel is then
//! the panel that ships, following the window list, rather than one whose
//! taskbar gave up at its start.
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

/// How long the Callgrind tool may take to replace Valgrind's launcher: a
/// guard against a hang, not a time it is expected to take, even loaded.
const TOOL_STARTS: Duration = Duration::from_secs(60);

/// What a measurement's program is granted, or why nothing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Grant {
    /// The component's grant, for this executable.
    Granted {
        component: &'static Component,
        executable: PathBuf,
    },
    /// The compositor restricts nothing (sway, labwc).
    Unrestricted(Compositor),
    /// A component that uses nothing restricted.
    NothingRestricted(&'static Component),
    /// The program is none of the shell's components, by its file name,
    /// and `--component` named none.
    NotAComponent(String),
}

impl Grant {
    /// What `request`'s program is granted, or why nothing.
    pub(crate) fn of(request: &Request) -> Result<Self, Error> {
        if request.compositor != Compositor::Kwin {
            return Ok(Self::Unrestricted(request.compositor));
        }
        let program = Path::new(&request.program);
        let Some(component) = crate::grant::component(program, request.component.as_deref())?
        else {
            return Ok(Self::NotAComponent(
                program
                    .file_name()
                    .unwrap_or(program.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
            ));
        };
        if component.wayland_grants.is_empty() {
            return Ok(Self::NothingRestricted(component));
        }
        let executable = if request.instructions {
            callgrind_tool()?
        } else {
            resolve(&request.program)?
        };
        Ok(Self::Granted {
            component,
            executable,
        })
    }

    /// Installs the grant, if there is one, into a session's data directory
    /// `data_home`.
    pub(crate) fn install(&self, data_home: &Path) -> Result<(), Error> {
        match self {
            Self::Granted {
                component,
                executable,
            } => crate::grant::install(component, executable, data_home).map(drop),
            _ => Ok(()),
        }
    }

    /// What a run's notes say was granted, or why nothing.
    pub(crate) fn note(&self) -> String {
        match self {
            Self::Granted {
                component,
                executable,
            } => crate::grant::describe(component, executable),
            Self::Unrestricted(compositor) => {
                format!("nothing: {} restricts nothing", compositor.name())
            }
            Self::NothingRestricted(component) => {
                format!("nothing: {} uses nothing restricted", component.package)
            }
            Self::NotAComponent(name) => format!(
                "NOTHING: {name} is not one of the shell's components by its file name; if it \
                 is one, --component PACKAGE names it, and it is granted as the shell grants it"
            ),
        }
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
/// run of `sleep` under it, ended at once, shows where it is. A probe that
/// exits first (a missing tool, a broken install) fails at once.
fn callgrind_tool() -> Result<PathBuf, Error> {
    let mut probe = Command::new("valgrind")
        .args([
            "--tool=callgrind",
            "--callgrind-out-file=/dev/null",
            "sleep",
            "600",
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
        if let Ok(Some(status)) = probe.try_wait() {
            break Err(Error::CallgrindToolNotFound {
                why: format!("valgrind --tool=callgrind exited ({status}) before it ran"),
            });
        }
        if Instant::now() >= deadline {
            break Err(Error::CallgrindToolNotFound {
                why: format!("no Callgrind tool ran within {TOOL_STARTS:?}"),
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
    use deskrinch_shell::{LAUNCHER, PANEL};

    use super::*;

    #[test]
    fn the_callgrind_tool_is_told_by_its_name() {
        assert!(is_callgrind_tool(Path::new(
            "/usr/libexec/valgrind/callgrind-amd64-linux"
        )));
        assert!(!is_callgrind_tool(Path::new("/usr/bin/valgrind")));
        assert!(!is_callgrind_tool(Path::new("/usr/bin/valgrind.bin")));
    }

    fn request(program: &str, component: Option<&str>, compositor: Compositor) -> Request {
        Request {
            compositor,
            component: component.map(str::to_owned),
            ..Request::new(program.into())
        }
    }

    #[test]
    fn a_component_is_granted_under_kwin_and_the_rest_say_why_not() {
        let sh = resolve(OsStr::new("sh")).unwrap();
        let sh = sh.to_str().unwrap();
        let grant = |program, component, compositor| {
            Grant::of(&request(program, component, compositor)).unwrap()
        };
        assert_eq!(
            grant(sh, Some("deskrinch-panel"), Compositor::Kwin),
            Grant::Granted {
                component: &PANEL,
                executable: PathBuf::from(sh),
            }
        );
        // A copy of the panel, renamed: the case that cost a series.
        let not_a_component = grant("/tmp/measure/panel-after", None, Compositor::Kwin);
        assert_eq!(not_a_component, Grant::NotAComponent("panel-after".into()));
        assert!(not_a_component.note().contains("--component PACKAGE"));
        let launcher = grant(sh, Some("deskrinch-launcher"), Compositor::Kwin);
        assert_eq!(launcher, Grant::NothingRestricted(&LAUNCHER));
        assert_eq!(
            launcher.note(),
            "nothing: deskrinch-launcher uses nothing restricted"
        );
        for compositor in [Compositor::Sway, Compositor::Labwc] {
            let unrestricted = grant(sh, Some("deskrinch-panel"), compositor);
            assert_eq!(unrestricted, Grant::Unrestricted(compositor));
            assert!(unrestricted.note().ends_with("restricts nothing"));
        }
    }

    #[test]
    fn the_note_says_what_was_granted_and_to_what() {
        let granted = Grant::Granted {
            component: &PANEL,
            executable: PathBuf::from("/tmp/measure/panel-after"),
        };
        assert!(
            granted
                .note()
                .starts_with("org_kde_plasma_window_management to /tmp/measure/panel-after")
        );
    }

    #[test]
    fn a_bare_name_is_found_on_the_path() {
        let sh = resolve(OsStr::new("sh")).unwrap();
        assert!(sh.is_absolute() && sh.is_file(), "{}", sh.display());
    }
}
