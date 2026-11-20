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

use deskrinch_shell::exit_status::stops_in_order_line;
use deskrinch_test_wait::wait_for;
use deskrinch_window_management::{Action, WindowManagement};
use rustix::process::{Pid, Signal, kill_process};

use crate::support::{
    Desktop, LAUNCHER, LAUNCHER_INTERFACE, LAUNCHER_PATH, outer_windows, start, start_logging,
};

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
fn close(desktop: Desktop) {
    close_saying(desktop);
}

/// [`close`], and all the desktop said, once it has ended.
fn close_saying(mut desktop: Desktop) -> String {
    let windows = outer_windows();
    for window in windows.windows() {
        windows.perform(window.id, Action::Close).unwrap();
    }
    let status = desktop.wait();
    let said = desktop.said();
    assert!(status.success(), "{status}; it said:\n{said}");
    said
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

/// A component asked to stop once it is up ends in order, with status 0,
/// and is not started again.
///
/// `SIGTERM` is sent only once the panel has said that a stop signal now
/// ends it in order (`stops_in_order_line`, its own words, from the moment
/// its handlers are in place): sent before, it would end the panel by the
/// signal, which is clean too but another case, and which of the two
/// happened would depend on how far the panel had got. The end by the
/// signal itself is the restart rules' (`deskrinch_shell::restart::Exit`),
/// tested there: no component the desktop starts goes without the handlers.
#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_component_ended_cleanly_is_not_started_again() {
    let desktop = start_logging("warn,deskrinch_component::stop=debug");
    let panel = the_process(&desktop, "deskrinch-panel");
    let pid = u32::try_from(panel.as_raw_nonzero().get()).unwrap();
    wait_said(&desktop, &stops_in_order_line(pid));
    // `SIGTERM`, a plain `kill`: a session's way of stopping a component.
    kill_process(panel, Signal::TERM).unwrap();
    wait_said(
        &desktop,
        "desktop: deskrinch-panel exited cleanly (exit status: 0); not started again, the \
         desktop runs on without it",
    );
    assert!(
        launcher_answers(&desktop).is_ok(),
        "the desktop runs on: {}",
        desktop.said()
    );
    // Not started again, by more than the desktop's word. The desktop says
    // its decision and, in the same call with nothing between that waits,
    // starts any component it starts again (`Components::ended`, a spawn):
    // after the launcher's answer, a round trip on the bus since that line
    // was read, a start would have a process.
    let panels = processes(&desktop.root, "deskrinch-panel");
    assert_eq!(panels, [], "a panel runs again: {}", desktop.said());
    let root = desktop.root.clone();
    let said = close_saying(desktop);
    // And nothing started it later: once the desktop has ended, everything
    // it said is in, and none of it after the clean exit starts the panel.
    let exit = said
        .find("deskrinch-panel exited cleanly")
        .expect("the clean exit, said");
    assert!(
        !said[exit..].contains("deskrinch-panel failed"),
        "the panel was started again:\n{said}"
    );
    assert_eq!(processes(&root, "deskrinch-panel"), []);
}
