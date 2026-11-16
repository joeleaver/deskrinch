//! `cargo xtask type`, the typist `measure --type` runs in its nested
//! session, refuses outside one, before it connects to anything; and
//! `measure --type` refuses to run without the two lines it reads. Neither
//! needs a compositor.

#[cfg(test)]
mod typist {
    use std::process::{Command, Stdio};

    #[test]
    fn the_typist_refuses_outside_a_nested_session() {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("type")
            .env_remove("DESKRINCH_TEST_COMPOSITOR")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(
            stderr.contains("types only into a nested session"),
            "{stderr}"
        );
        assert!(output.stdout.is_empty(), "it said it was ready");
    }

    #[test]
    fn the_typist_is_not_in_the_help() {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("--help")
            .output()
            .unwrap();
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{help}");
        assert!(help.contains("measure"), "{help}");
        assert!(!help.contains("  type "), "{help}");
    }

    #[test]
    fn typing_needs_both_lines_and_a_summon() {
        let measure = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_xtask"))
                .arg("measure")
                .args(args)
                .arg("program")
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };
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
}
