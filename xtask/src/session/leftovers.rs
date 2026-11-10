//! Finding what is left of a session after it has been ended.
//!
//! Ending the session's process group ends everything that stayed in it. A
//! process that left the group (`setsid`, a double fork into a new session)
//! still carries the session's `XDG_RUNTIME_DIR` in its environment, which
//! no other process has: the directory is fresh and private.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use rustix::process::{Pid, RawPid};

/// A process of the session that is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Leftover {
    pub(crate) pid: Pid,
    pub(crate) command: String,
}

impl fmt::Display for Leftover {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.pid.as_raw_nonzero(), self.command)
    }
}

/// The processes whose environment names `runtime` as `XDG_RUNTIME_DIR`.
/// Only processes the caller may read are seen: the user's own, which is
/// all a session can start.
pub(super) fn find(runtime: &Path) -> Vec<Leftover> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: RawPid = entry.file_name().to_str()?.parse().ok()?;
            let environ = fs::read(entry.path().join("environ")).ok()?;
            if !names_runtime(&environ, runtime) {
                return None;
            }
            let cmdline = fs::read(entry.path().join("cmdline")).unwrap_or_default();
            Some(Leftover {
                pid: Pid::from_raw(pid)?,
                command: command_line(&cmdline),
            })
        })
        .collect()
}

/// Whether a NUL-separated environment sets `XDG_RUNTIME_DIR` to `runtime`.
fn names_runtime(environ: &[u8], runtime: &Path) -> bool {
    let wanted: Vec<u8> = [b"XDG_RUNTIME_DIR=", runtime.as_os_str().as_bytes()].concat();
    environ
        .split(|&byte| byte == 0)
        .any(|entry| entry == wanted)
}

/// A NUL-separated command line, readable.
fn command_line(cmdline: &[u8]) -> String {
    cmdline
        .split(|&byte| byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| OsStr::from_bytes(argument).to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn matches_the_exact_runtime_directory() {
        let runtime = Path::new("/tmp/deskrinch-session-abc/run");
        let environ = b"HOME=/home/x\0XDG_RUNTIME_DIR=/tmp/deskrinch-session-abc/run\0";
        assert!(names_runtime(environ, runtime));
        let other = b"XDG_RUNTIME_DIR=/tmp/deskrinch-session-abc/run2\0";
        assert!(!names_runtime(other, runtime));
        let prefix = b"XDG_RUNTIME_DIR=/tmp/deskrinch-session-abc\0";
        assert!(!names_runtime(prefix, runtime));
        assert!(!names_runtime(b"", runtime));
    }

    #[test]
    fn reads_a_command_line() {
        assert_eq!(command_line(b"sleep\x00600\x00"), "sleep 600");
        assert_eq!(command_line(b""), "");
    }

    #[test]
    fn finds_a_process_by_its_runtime_directory() {
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let runtime = dir.path().join("run");
        assert_eq!(find(&runtime), []);
        let mut child = Command::new("sleep")
            .arg("60")
            .env("XDG_RUNTIME_DIR", &runtime)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        // Until the child has exec'd, its environment is still this process's.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut found = find(&runtime);
        while found.is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            found = find(&runtime);
        }
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].pid.as_raw_nonzero().get(),
            i32::try_from(child.id()).unwrap()
        );
        assert_eq!(found[0].command, "sleep 60");
    }
}
