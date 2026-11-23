//! The hidden subcommands that act on a compositor refuse outside a nested
//! session, before they start or connect to anything: run there, they
//! would end the real desktop's compositor (`in-session`), or type into
//! and point at the real desktop (`input`). Plain `cargo test` runs these,
//! anywhere: what they run only refuses, and would reach nothing if it
//! did not (`deskrinch_test_dbus::session::outside`).

#[cfg(test)]
mod refusals {
    use std::process::{Command, Output, Stdio};

    use deskrinch_test_dbus::session::outside;

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
}
