use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use clap::Parser;

use super::{Args, Then, Until, shell_status};

/// `Args` as `cargo xtask in-session` parses them.
#[derive(Debug, Parser)]
struct Command {
    #[command(flatten)]
    args: Args,
}

fn parsed(line: &[&str]) -> Result<Args, clap::Error> {
    let mut words = vec!["in-session"];
    words.extend_from_slice(line);
    Command::try_parse_from(words).map(|command| command.args)
}

#[test]
fn the_wait_the_action_and_the_component_with_its_arguments_are_read() {
    let args = parsed(&[
        "--dir",
        "/d",
        "--until",
        "prepared",
        "--then",
        "kill-compositor",
        "/bin/component",
        "--show",
        "-x",
    ])
    .unwrap();
    assert_eq!(args.dir.to_str(), Some("/d"));
    assert_eq!(args.until, Until::Prepared);
    assert_eq!(args.then, Then::KillCompositor);
    assert_eq!(args.component.to_str(), Some("/bin/component"));
    assert_eq!(args.arguments, ["--show", "-x"]);
}

#[test]
fn a_wait_or_an_action_it_does_not_know_is_refused() {
    let base = ["--dir", "/d", "--until", "presented", "--then"];
    assert!(parsed(&[&base[..], &["kill-compositor", "/c"]].concat()).is_ok());
    let then = |word: &str| parsed(&[&base[..], &[word, "/c"]].concat()).unwrap().then;
    assert_eq!(then("signal:TERM"), Then::SignalTerm);
    assert_eq!(then("signal:INT"), Then::SignalInt);
    assert!(parsed(&[&base[..], &["signal:KILL", "/c"]].concat()).is_err());
    assert!(parsed(&[&base[..], &["reboot", "/c"]].concat()).is_err());
    assert!(
        parsed(&[
            "--dir",
            "/d",
            "--until",
            "shown",
            "--then",
            "kill-compositor",
            "/c"
        ])
        .is_err()
    );
}

#[test]
fn the_component_is_needed() {
    assert!(
        parsed(&[
            "--dir",
            "/d",
            "--until",
            "presented",
            "--then",
            "kill-compositor"
        ])
        .is_err()
    );
}

#[test]
fn a_presented_frame_is_told_from_a_prepared_one() {
    let presented = "[PERF] frame 1: style 0.1ms layout 0.2ms paint 0.3ms present 0.4ms | x=1";
    let prepared = "[PERF] prepared 1: style 0.1ms layout 0.2ms paint 0.3ms | x=1";
    let log = format!("starting\n{presented}\n");
    assert!(Until::Presented.in_log(&log));
    assert!(!Until::Prepared.in_log(&log));
    let log = format!("starting\n{prepared}\n");
    assert!(Until::Prepared.in_log(&log));
    assert!(!Until::Presented.in_log(&log));
    assert!(!Until::Presented.in_log("no frame yet\n"));
}

#[test]
fn a_status_is_given_as_a_shell_gives_it() {
    assert_eq!(shell_status(ExitStatus::from_raw(0)), Some(0));
    assert_eq!(shell_status(ExitStatus::from_raw(69 << 8)), Some(69));
    // Ended by SIGTERM (15).
    assert_eq!(shell_status(ExitStatus::from_raw(15)), Some(143));
}

#[test]
fn the_waits_on_the_bus_need_the_name() {
    for until in ["owned", "notified"] {
        let without = [
            "--dir",
            "/d",
            "--until",
            until,
            "--then",
            "kill-compositor",
            "/c",
        ];
        assert!(parsed(&without).is_err(), "{until} without --name");
        let with = [
            "--dir",
            "/d",
            "--until",
            until,
            "--name",
            "org.freedesktop.Notifications",
            "--then",
            "kill-compositor",
            "/c",
        ];
        let args = parsed(&with).unwrap();
        assert_eq!(args.name.as_deref(), Some("org.freedesktop.Notifications"));
    }
}

#[test]
fn notified_waits_for_a_presented_frame_and_owned_for_none() {
    let presented =
        "[PERF] window 1 frame 1: style 0.1ms layout 0.2ms paint 0.3ms present 0.4ms | x=1";
    assert!(Until::Notified.in_log(presented));
    assert!(!Until::Owned.in_log(presented));
}
