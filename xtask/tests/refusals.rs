//! The hidden subcommands that act on a compositor refuse outside a nested
//! session, before they start or connect to anything: run there, they
//! would end the real desktop's compositor (`in-session`), type into and
//! point at the real desktop (`input`), or open and close hundreds of
//! windows on it (`busy-compositor`). `systemd-scope-tests` refuses
//! off a CI runner: run there, it would start a unit in the user's own
//! systemd. Plain `cargo test` runs these, anywhere: what they run only
//! refuses, and would reach nothing if it did not
//! (`deskrinch_test_dbus::session::outside`).

#[cfg(test)]
mod refusals {
    use std::process::{Command, Output, Stdio};

    use deskrinch_test_dbus::session::outside;
    use deskrinch_test_session::ci::CI_VARIABLE;

    /// `cargo xtask` with `args`, [`outside`] any session, its standard
    /// input closed.
    fn outside_a_session(args: &[&str]) -> Output {
        let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"));
        xtask.args(args).stdin(Stdio::null());
        let _root = outside(&mut xtask).unwrap();
        xtask.output().unwrap()
    }

    #[test]
    fn in_session_refuses_and_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let dir_arg = dir.path().to_str().unwrap();
        let mark = dir.path().join("started");
        let ran = outside_a_session(&[
            "in-session",
            "--dir",
            dir_arg,
            "--until",
            "presented",
            "--then",
            "kill-compositor",
            "/bin/sh",
            "-c",
            "touch \"$0\"",
            mark.to_str().unwrap(),
        ]);
        let said = String::from_utf8_lossy(&ran.stderr);
        assert!(!ran.status.success(), "it did not fail; it said:\n{said}");
        assert!(
            said.contains("runs only inside cargo xtask session"),
            "it did not refuse; it said:\n{said}"
        );
        assert!(!mark.exists(), "the component was started");
        assert!(
            !dir.path().join("stderr").exists(),
            "the component's stderr file was made"
        );
    }

    #[test]
    fn input_refuses_and_connects_to_nothing() {
        let ran = outside_a_session(&["input"]);
        let said = String::from_utf8_lossy(&ran.stderr);
        assert!(!ran.status.success(), "it did not fail; it said:\n{said}");
        assert!(
            said.contains("types and points only in a nested session"),
            "it did not refuse; it said:\n{said}"
        );
        // It said nothing on its standard output: no "ready", so no
        // keyboard or pointer was made.
        assert!(
            ran.stdout.is_empty(),
            "{:?}",
            String::from_utf8_lossy(&ran.stdout)
        );
    }

    /// Without its refusal it would open windows on whatever display it
    /// found; [`outside`] leaves it none, so it would fail to connect.
    #[test]
    fn busy_compositor_refuses_and_connects_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let done = dir.path().join("done");
        let ran = outside_a_session(&[
            "busy-compositor",
            "--rounds",
            "400",
            "--done",
            done.to_str().unwrap(),
            "--within",
            "60",
        ]);
        let said = String::from_utf8_lossy(&ran.stderr);
        assert!(!ran.status.success(), "it did not fail; it said:\n{said}");
        assert!(
            said.contains(
                "`cargo xtask busy-compositor` runs only in a nested session xtask started, as \
                 outside one it would load the real desktop: not inside an isolated session"
            ),
            "it did not refuse; it said:\n{said}"
        );
        assert!(!done.exists(), "it wrote that the compositor was done");
    }

    /// Off a CI runner (the CI variable removed, so that this holds on CI
    /// too) and outside any session. Without its refusal the command would
    /// run the test, which refuses itself, and finds no systemd here.
    #[test]
    fn systemd_scope_tests_refuses_off_a_runner_and_runs_nothing() {
        let mut xtask = Command::new(env!("CARGO_BIN_EXE_xtask"));
        xtask.arg("systemd-scope-tests").stdin(Stdio::null());
        let _root = outside(&mut xtask).unwrap();
        xtask.env_remove(CI_VARIABLE);
        let ran = xtask.output().unwrap();
        let said = String::from_utf8_lossy(&ran.stderr);
        assert!(!ran.status.success(), "it did not fail; it said:\n{said}");
        assert!(
            said.contains(
                "systemd-scope-tests (which starts a unit in the user's systemd) runs only on a \
                 CI runner (GITHUB_ACTIONS=true), and here GITHUB_ACTIONS is not set"
            ),
            "it did not refuse; it said:\n{said}"
        );
        assert!(
            ran.stdout.is_empty(),
            "it ran the test: {:?}",
            String::from_utf8_lossy(&ran.stdout)
        );
    }
}
