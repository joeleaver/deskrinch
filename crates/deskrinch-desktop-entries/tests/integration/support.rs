//! Fixtures shared by the tests.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_desktop_entries::{DesktopEntry, DesktopId, Environment};

/// How long a test waits for something asynchronous before failing.
pub(crate) const PATIENCE: Duration = Duration::from_secs(10);

/// Writes `text` to `dir/relative`, creating directories, through a rename
/// so a watcher never sees a half-written file.
pub(crate) fn write(dir: &Path, relative: &str, text: &str) -> PathBuf {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let temporary = path.with_extension("partial");
    fs::write(&temporary, text).unwrap();
    fs::rename(&temporary, &path).unwrap();
    path
}

/// Writes an executable shell script.
pub(crate) fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// An environment whose only data directories are `home` and `system`.
pub(crate) fn environment(home: &Path, system: &[&Path]) -> Environment {
    Environment {
        data_home: Some(home.to_path_buf()),
        data_dirs: system.iter().map(|dir| dir.to_path_buf()).collect(),
        ..Environment::default()
    }
}

/// An application entry parsed from `text`.
pub(crate) fn entry(dir: &Path, id: &str, text: &str) -> DesktopEntry {
    let path = write(dir, id, text);
    DesktopEntry::parse(DesktopId::new(id).unwrap(), path, text).unwrap()
}

/// The recorder: a script that writes what it was run with to
/// `$DESKRINCH_TEST_OUT/<its pid>`, one `key=value` per line.
pub(crate) fn recorder(dir: &Path) -> PathBuf {
    let path = dir.join("recorder");
    script(
        &path,
        r#"out="$DESKRINCH_TEST_OUT/$$"
{
  for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
  printf 'cwd=%s\n' "$(pwd)"
  printf 'token=%s\n' "${XDG_ACTIVATION_TOKEN-unset}"
  printf 'startup=%s\n' "${DESKTOP_STARTUP_ID-unset}"
  printf 'extra=%s\n' "$DESKRINCH_TEST_EXTRA"
  read -r _pid _comm _state ppid _pgrp sid _rest < /proc/$$/stat
  printf 'ppid=%s\nsid=%s\n' "$ppid" "$sid"
} > "$out.partial"
mv "$out.partial" "$out"
"#,
    );
    path
}

/// What the recorder with PID `pid` wrote, once it has: each key's values.
pub(crate) fn recorded(out: &Path, pid: u32) -> HashMap<String, Vec<String>> {
    let path = out.join(pid.to_string());
    let text = wait_for(|| fs::read_to_string(&path).ok());
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').unwrap();
        values.entry(key.into()).or_default().push(value.into());
    }
    values
}

/// Polls `probe` until it returns something, or fails the test.
pub(crate) fn wait_for<T>(mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A private `dbus-daemon` with nothing to activate, stopped on drop.
pub(crate) struct PrivateBus {
    daemon: Child,
    /// Its address.
    pub(crate) address: String,
    _dir: tempfile::TempDir,
}

impl PrivateBus {
    pub(crate) fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("bus.conf");
        fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={}/socket</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                dir.path().display()
            ),
        )
        .unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("the D-Bus tests need dbus-daemon");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            daemon,
            address: address.trim().to_owned(),
            _dir: dir,
        }
    }

    /// A connection to the bus that owns `name` and serves `object` at
    /// `path`.
    pub(crate) fn serve<I: zbus::object_server::Interface>(
        &self,
        name: &str,
        path: &str,
        object: I,
    ) -> zbus::Connection {
        zbus::block_on(async {
            zbus::connection::Builder::address(self.address.as_str())
                .name(name)
                .serve_at(path, object)
                .build()
                .await
        })
        .unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        self.daemon.kill().ok();
        self.daemon.wait().ok();
    }
}
