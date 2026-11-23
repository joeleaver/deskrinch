//! `cargo xtask input`, the input client measure runs in its nested
//! session, refuses outside one, before it connects to anything; and
//! measure refuses typing without the two lines it reads, and pointer
//! options that do not make sense, before any session starts. None of it
//! needs a compositor.

#[cfg(test)]
mod outside;

#[cfg(test)]
mod input {
    use std::process::{Command, Output, Stdio};

    use crate::outside::outside;

    /// `xtask measure` with `args`, of a program it never gets to start.
    fn measure(args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("measure")
            .args(args)
            .arg("program")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// Run [`outside`] any session, so that it would find nothing to type
    /// into if it did not refuse.
    #[test]
    fn the_input_client_refuses_outside_a_nested_session() {
        let mut input = Command::new(env!("CARGO_BIN_EXE_xtask"));
        input.arg("input").stdin(Stdio::null());
        let _root = outside(&mut input);
        let output = input.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("types and points only in a nested session"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "it said it was ready");
    }

    /// The compositor's name alone does not make a session: the client asks
    /// the session's own check (`deskrinch_test_session::check`), and says
    /// its reason, here that there is no session root. Run [`outside`] any
    /// session but for the name.
    #[test]
    fn the_input_client_refuses_the_compositor_s_name_alone() {
        let mut input = Command::new(env!("CARGO_BIN_EXE_xtask"));
        input.arg("input").stdin(Stdio::null());
        let _root = outside(&mut input);
        input.env(deskrinch_test_session::COMPOSITOR_VARIABLE, "kwin");
        let output = input.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("types and points only in a nested session"),
            "{stderr}"
        );
        assert!(
            stderr.contains("DESKRINCH_SESSION_DIR is not set"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "it said it was ready");
    }

    #[test]
    fn the_input_client_is_not_in_the_help() {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("--help")
            .output()
            .unwrap();
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{help}");
        assert!(help.contains("measure"), "{help}");
        assert!(!help.contains("  input "), "{help}");
    }

    #[test]
    fn typing_needs_both_lines_and_a_summon() {
        let lines = [
            "--keyboard-line",
            "has the keyboard",
            "--query-line",
            "{} typed",
        ];
        let summon = ["--summon", "show", "--dismiss", "hide"];
        let without_lines = measure(&[&summon[..], &["--type", "plain"]].concat());
        let stderr = String::from_utf8_lossy(&without_lines.stderr);
        assert_eq!(without_lines.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("--keyboard-line"), "{stderr}");
        assert!(stderr.contains("--query-line"), "{stderr}");
        let without_summon = measure(&[&["--type", "plain"][..], &lines].concat());
        let stderr = String::from_utf8_lossy(&without_summon.stderr);
        assert_eq!(without_summon.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("--summon"), "{stderr}");
        let no_place = ["--keyboard-line", "k", "--query-line", "typed"];
        let wrong_pattern = measure(&[&summon[..], &["--type", "plain"], &no_place].concat());
        let stderr = String::from_utf8_lossy(&wrong_pattern.stderr);
        assert_eq!(wrong_pattern.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("must hold {} once"), "{stderr}");
        let untypable = measure(&[&summon[..], &["--type", "Plain"], &lines].concat());
        let stderr = String::from_utf8_lossy(&untypable.stderr);
        assert_eq!(untypable.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("'P' cannot be typed"), "{stderr}");
    }

    #[test]
    fn the_pointer_needs_a_placing_and_a_move_and_points_on_the_outputs() {
        let alone = measure(&["--place-pointer", "640,360"]);
        let stderr = String::from_utf8_lossy(&alone.stderr);
        assert_eq!(alone.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("--move-pointer"), "{stderr}");
        let unplaced = measure(&["--move-pointer", "640,700"]);
        let stderr = String::from_utf8_lossy(&unplaced.stderr);
        assert_eq!(unplaced.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("--place-pointer"), "{stderr}");
        let not_a_point = measure(&["--place-pointer", "640", "--move-pointer", "1,2"]);
        let stderr = String::from_utf8_lossy(&not_a_point.stderr);
        assert_eq!(not_a_point.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("is not X,Y or N:X,Y"), "{stderr}");
        // Refused by measure itself, before any session: one output asked.
        let past = measure(&[
            "--outputs",
            "1",
            "--place-pointer",
            "640,360",
            "--move-pointer",
            "2:10,10",
        ]);
        let stderr = String::from_utf8_lossy(&past.stderr);
        assert_eq!(past.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("2:10,10 is on an output past the session's 1"),
            "{stderr}"
        );
        assert!(
            !stderr.contains("measure: run"),
            "a session started: {stderr}"
        );
    }
}
