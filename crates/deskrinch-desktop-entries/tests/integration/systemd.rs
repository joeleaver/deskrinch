//! A launch into a real transient scope of the real systemd user instance.
//!
//! Ignored by default: it needs the user's own systemd and session bus.
//! `cargo xtask systemd-scope-tests` runs it. It starts only the recorder
//! script (and a `sleep` the recorder forks), in a transient scope it stops
//! at the end.

use std::path::Path;

use deskrinch_desktop_entries::{Environment, LaunchContext, Launched, Placement, launch};
use deskrinch_test_wait::eventually;

use crate::support::{entry, recorded, recorder};

/// The last line of `/proc/<pid>/cgroup`: the unified hierarchy's path.
fn cgroup(text: &str) -> &str {
    text.lines().last().unwrap_or_default()
}

#[test]
#[ignore = "needs the real systemd user instance; run by `cargo xtask systemd-scope-tests`"]
fn a_launch_and_its_children_land_in_a_real_transient_scope() {
    let env = Environment::from_process();
    let bus = env.session_bus.clone().expect("a session bus");
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=deskrinch scope test\nExec={}\n",
        recorder(dir.path())
    );
    let app = entry(dir.path(), "org.deskrinch.ScopeTest.desktop", &text);
    let context = LaunchContext::new(&env)
        .with_scope_launcher("deskrinch")
        .with_env("DESKRINCH_TEST_OUT", dir.path())
        .with_env("DESKRINCH_TEST_HOLD", "60");
    let launched = launch(&app, None, &[], &context).unwrap();
    let [Launched::Process { pid, placement }] = &launched[..] else {
        panic!("one process expected: {launched:?}");
    };
    let Placement::Scope { unit } = placement else {
        panic!("not in a scope: {placement:?}");
    };
    assert!(
        unit.starts_with("app-deskrinch-org.deskrinch.ScopeTest-"),
        "{unit}"
    );

    // The process, and the child it forked after it started, are both in
    // the scope: from the recorder's own reading, and from ours.
    let record = recorded(dir.path(), *pid);
    let suffix = format!("/app.slice/{unit}");
    assert!(record["cgroup"][0].ends_with(&suffix), "{record:?}");
    assert!(record["child_cgroup"][0].ends_with(&suffix), "{record:?}");
    for process in [pid.to_string(), record["child"][0].clone()] {
        let text = std::fs::read_to_string(format!("/proc/{process}/cgroup")).unwrap();
        assert!(cgroup(&text).ends_with(&suffix), "{process}: {text}");
    }

    stop(&bus, unit);
    for process in [pid.to_string(), record["child"][0].clone()] {
        let proc = format!("/proc/{process}");
        eventually(&format!("{proc} to go"), || !Path::new(&proc).exists());
    }
}

/// Stops `unit` through systemd's manager, killing what is in it.
fn stop(bus: &str, unit: &str) {
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(bus).build().await?;
        connection
            .call_method(
                Some("org.freedesktop.systemd1"),
                "/org/freedesktop/systemd1",
                Some("org.freedesktop.systemd1.Manager"),
                "StopUnit",
                &(unit, "replace"),
            )
            .await
    })
    .unwrap();
}
