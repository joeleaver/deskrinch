//! A launch into a real transient scope of the real systemd user instance.
//!
//! Ignored by default: it needs the user's own systemd and session bus.
//! `cargo xtask systemd-scope-tests` runs it, on CI. It starts only the
//! recorder script (and a `sleep` the recorder forks), in a transient scope
//! it stops at the end.
//!
//! It runs only on a CI runner, and refuses before it touches anything
//! anywhere else: on a developer's machine it would start a transient
//! scope unit in their own systemd user instance. The refusal is tested in
//! the two forms that are harmless when it fails: in-process, and in a
//! child of this binary outside any session, where no systemd is reachable.

use std::path::Path;
use std::process::{Command, Stdio};

use deskrinch_desktop_entries::{Environment, LaunchContext, Launched, Placement, launch};
use deskrinch_test_dbus::session::outside;
use deskrinch_test_session::ci::{CI_VARIABLE, Ci, OnARunner};
use deskrinch_test_wait::eventually;

use crate::support::{entry, recorded, recorder};

/// The last line of `/proc/<pid>/cgroup`: the unified hierarchy's path.
fn cgroup(text: &str) -> &str {
    text.lines().last().unwrap_or_default()
}

#[test]
#[ignore = "needs the real systemd user instance; run by `cargo xtask systemd-scope-tests`"]
fn a_launch_and_its_children_land_in_a_real_transient_scope() {
    let runner = refuse_off_a_runner(&Ci::read());
    launch_into_a_real_scope(runner);
}

/// The test's body, on a CI runner: everything from its first touch of the
/// bus.
fn launch_into_a_real_scope(_runner: OnARunner) {
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

/// Panics unless `ci` says this is certainly a CI runner: anywhere else the
/// test would start a unit in the user's own systemd.
fn refuse_off_a_runner(ci: &Ci) -> OnARunner {
    ci.only_on_a_runner().unwrap_or_else(|why| {
        panic!(
            "the systemd scope test {why}: anywhere else it would start a unit in \
             your own systemd user instance"
        )
    })
}

#[test]
#[should_panic(expected = "the systemd scope test runs only on a CI runner")]
fn off_a_runner_the_test_refuses() {
    let _runner = refuse_off_a_runner(&Ci::from_value(None));
}

#[test]
fn on_a_runner_the_test_goes_on() {
    let _runner = refuse_off_a_runner(&Ci::from_value(Some("true".as_ref())));
}

/// The test itself, run in a child of this binary outside any session and
/// off CI: it refuses before it touches anything. Without the refusal it
/// would find a session bus address that leads nowhere, and an empty
/// runtime directory, so no systemd.
#[test]
fn run_off_a_runner_the_test_refuses_before_anything() {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--ignored",
            "--exact",
            "systemd::a_launch_and_its_children_land_in_a_real_transient_scope",
        ])
        .stdin(Stdio::null());
    let _root = outside(&mut child).unwrap();
    child.env_remove(CI_VARIABLE);
    let output = child.output().unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("1 failed"), "it did not fail: {said}");
    assert!(
        said.contains(
            "the systemd scope test runs only on a CI runner (GITHUB_ACTIONS=true), and here \
             GITHUB_ACTIONS is not set"
        ),
        "{said}"
    );
}
