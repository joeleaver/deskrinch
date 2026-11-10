//! Fixtures shared by the tests.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_desktop_entries::{BaseDirs, DesktopEntry, DesktopId, Environment, LaunchContext};
use deskrinch_test_dbus::session::REAL_SESSION_VARIABLES;

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
        base_dirs: BaseDirs {
            data_home: Some(home.to_path_buf()),
            data_dirs: system.iter().map(|dir| dir.to_path_buf()).collect(),
            ..BaseDirs::default()
        },
        ..Environment::default()
    }
}

/// `context` launching without any of the real session's variables, for
/// tests that run no session of their own.
pub(crate) fn without_real_session(context: LaunchContext) -> LaunchContext {
    REAL_SESSION_VARIABLES
        .iter()
        .fold(context, |context, variable| context.without_env(*variable))
}

/// An application entry parsed from `text`.
pub(crate) fn entry(dir: &Path, id: &str, text: &str) -> DesktopEntry {
    let path = write(dir, id, text);
    DesktopEntry::parse(DesktopId::new(id).unwrap(), path, text).unwrap()
}

/// The recorder: a script that writes what it was run with to
/// `$DESKRINCH_TEST_OUT/<its pid>`, one `key=value` per line. With
/// `$DESKRINCH_TEST_HOLD` set, it also forks a child, records the child's
/// PID and both cgroups, and then waits for the child (which sleeps that
/// many seconds).
pub(crate) fn recorder(dir: &Path) -> PathBuf {
    let path = dir.join("recorder");
    script(
        &path,
        r#"out="$DESKRINCH_TEST_OUT/$$"
child=
if [ -n "$DESKRINCH_TEST_HOLD" ]; then
  sleep "$DESKRINCH_TEST_HOLD" &
  child=$!
fi
{
  for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
  printf 'cwd=%s\n' "$(pwd)"
  printf 'token=%s\n' "${XDG_ACTIVATION_TOKEN-unset}"
  printf 'startup=%s\n' "${DESKTOP_STARTUP_ID-unset}"
  printf 'extra=%s\n' "$DESKRINCH_TEST_EXTRA"
  awk 'BEGIN { for (name in ENVIRON) printf "var=%s\n", name }'
  printf 'bus=%s\n' "${DBUS_SESSION_BUS_ADDRESS-unset}"
  read -r _pid _comm _state ppid _pgrp sid _rest < /proc/$$/stat
  printf 'ppid=%s\nsid=%s\n' "$ppid" "$sid"
  printf 'cgroup=%s\n' "$(cat /proc/$$/cgroup)"
  if [ -n "$child" ]; then
    printf 'child=%s\n' "$child"
    printf 'child_cgroup=%s\n' "$(cat /proc/$child/cgroup)"
  fi
} > "$out.partial"
mv "$out.partial" "$out"
if [ -n "$child" ]; then wait "$child"; fi
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

/// The test's own bus (see `deskrinch-test-dbus`), stopped when dropped.
pub(crate) struct PrivateBus(deskrinch_test_dbus::PrivateBus);

impl PrivateBus {
    /// A bus that activates nothing.
    pub(crate) fn start() -> Self {
        Self::with_services(&[])
    }

    /// A bus that activates the services described in `service_dirs`.
    pub(crate) fn with_services(service_dirs: &[&Path]) -> Self {
        let bus = deskrinch_test_dbus::PrivateBus::start_with_services(service_dirs);
        Self(bus.expect("a private bus (needs dbus-daemon)"))
    }

    pub(crate) fn address(&self) -> &str {
        self.0.address()
    }

    /// `context` with the variables that point what it launches at this
    /// bus's isolated session (its XDG directories and the bus).
    pub(crate) fn isolate(&self, context: LaunchContext) -> LaunchContext {
        let session = self.0.session_env();
        let context = REAL_SESSION_VARIABLES
            .iter()
            .filter(|variable| !session.iter().any(|(name, _)| name == *variable))
            .fold(context, |context, variable| context.without_env(*variable));
        session.into_iter().fold(context, |context, (name, value)| {
            context.with_env(name, value)
        })
    }

    /// A connection to this bus that owns `name` and serves `object` at
    /// `path`.
    pub(crate) fn serve<I: zbus::object_server::Interface>(
        &self,
        name: &str,
        path: &str,
        object: I,
    ) -> zbus::Connection {
        serve(self.address(), name, path, object)
    }
}

/// A connection to the bus at `address` that owns `name` and serves
/// `object` at `path`.
pub(crate) fn serve<I: zbus::object_server::Interface>(
    address: &str,
    name: &str,
    path: &str,
    object: I,
) -> zbus::Connection {
    serve_all(address, name, |builder| builder.serve_at(path, object))
}

/// A connection to the bus at `address` that serves what `objects` adds to
/// its builder, and only then owns `name`.
///
/// A client waits for the name before it calls, so every object must be in
/// place before the name is claimed: `Builder::build` serves the builder's
/// objects first and requests the name last. An object added to the
/// connection after `build` would race the client's first call.
pub(crate) fn serve_all<'a>(
    address: &'a str,
    name: &'a str,
    objects: impl FnOnce(zbus::connection::Builder<'a>) -> zbus::connection::Builder<'a>,
) -> zbus::Connection {
    zbus::block_on(async {
        objects(zbus::connection::Builder::address(address))
            .name(name)
            .build()
            .await
    })
    .unwrap()
}
