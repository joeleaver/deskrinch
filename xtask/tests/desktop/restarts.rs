//! A component that fails is started again, the way it was first started,
//! and one that ends cleanly is not: the nested desktop following each
//! component's restart policy (`deskrinch_shell::restart`).
//!
//! A component's process is found by its executable's name and the nested
//! session's directory in its environment (`DESKRINCH_SESSION_DIR`, which
//! every client of the session has). Each wait ends on what the desktop
//! says it decided, then on what that decision leads to.

use std::fs;
use std::path::Path;

use deskrinch_test_wait::wait_for;
use deskrinch_window_management::{Action, WindowManagement};
use rustix::process::{Pid, Signal, kill_process};

use crate::support::{Desktop, LAUNCHER, LAUNCHER_INTERFACE, LAUNCHER_PATH, outer_windows, start};

/// The processes of the nested session at `root` running `package`'s
/// binary.
fn processes(root: &Path, package: &str) -> Vec<Pid> {
    let session = format!("DESKRINCH_SESSION_DIR={}", root.display());
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let exe = fs::read_link(entry.path().join("exe")).ok()?;
            if exe.file_name()? != package {
                return None;
            }
            let environ = fs::read(entry.path().join("environ")).ok()?;
            environ
                .split(|&byte| byte == 0)
                .any(|variable| variable == session.as_bytes())
                .then(|| Pid::from_raw(pid))
                .flatten()
        })
        .collect()
}

/// The one process of `package` in the desktop's session.
fn the_process(desktop: &Desktop, package: &str) -> Pid {
    let found = processes(&desktop.root, package);
    assert_eq!(found.len(), 1, "one {package}: {found:?}");
    found[0]
}

/// Whether the launcher answers on the desktop's bus: owns its name and
/// says whether it is shown.
fn launcher_answers(desktop: &Desktop) -> Result<bool, String> {
    zbus::block_on(async {
        let connection = desktop.connect().await;
        let reply = connection
            .call_method(
                Some(LAUNCHER),
                LAUNCHER_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(LAUNCHER_INTERFACE, "Shown"),
            )
            .await
            .map_err(|error| error.to_string())?;
        let value: zbus::OwnedVariant = reply.body().deserialize().map_err(|e| e.to_string())?;
        bool::try_from(value).map_err(|error| error.to_string())
    })
}

/// Waits until the desktop has said `line`.
fn wait_said(desktop: &Desktop, line: &str) {
    wait_for(&format!("the desktop to say {line:?}"), || {
        let said = desktop.said();
        if said.contains(line) {
            Ok(())
        } else {
            Err(said)
        }
    });
}

/// Closes the desktop's window and checks that it ended well.
fn close(mut desktop: Desktop) {
    let windows = outer_windows();
    for window in windows.windows() {
        windows.perform(window.id, Action::Close).unwrap();
    }
    let status = desktop.wait();
    assert!(status.success(), "{status}; it said:\n{}", desktop.said());
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_component_killed_comes_back_and_is_ready_again() {
    let desktop = start(&[], &[]);
    let first = the_process(&desktop, "deskrinch-launcher");
    kill_process(first, Signal::KILL).unwrap();
    wait_said(
        &desktop,
        "desktop: deskrinch-launcher failed (signal: 9 (SIGKILL)); starting it again (start 2 \
         of at most 3 in 60 s)",
    );
    let again = wait_for("the launcher to run again", || {
        match processes(&desktop.root, "deskrinch-launcher").as_slice() {
            [pid] if *pid != first => Ok(*pid),
            other => Err(other.to_vec()),
        }
    });
    assert_ne!(again, first);
    let shown = wait_for("the launcher to answer on the bus again", || {
        launcher_answers(&desktop)
    });
    assert!(!shown, "started again hidden, as at first");
    close(desktop);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_component_ended_cleanly_is_not_started_again() {
    let desktop = start(&[], &[]);
    let panel = the_process(&desktop, "deskrinch-panel");
    // `SIGTERM`, a plain `kill`: a clean end, as systemd's on-failure has it.
    kill_process(panel, Signal::TERM).unwrap();
    wait_said(
        &desktop,
        "desktop: deskrinch-panel exited cleanly (signal: 15 (SIGTERM)); not started again, the \
         desktop runs on without it",
    );
    // Said once the decision is made; a start would follow it, not precede.
    assert_eq!(processes(&desktop.root, "deskrinch-panel"), []);
    assert!(
        launcher_answers(&desktop).is_ok(),
        "the desktop runs on: {}",
        desktop.said()
    );
    close(desktop);
}
