//! Launching processes, without a session bus (so without scopes).

use std::path::Path;

use deskrinch_desktop_entries::{
    DirectReason, Environment, LaunchContext, LaunchError, Launched, Placement, Target, launch,
};

use crate::support::{entry, recorded, recorder};

/// A context with no session bus, recording into `out`.
fn context(out: &Path) -> LaunchContext {
    LaunchContext::new(&Environment::default()).with_env("DESKRINCH_TEST_OUT", out)
}

fn pids(launched: &[Launched]) -> Vec<u32> {
    launched
        .iter()
        .map(|launched| match launched {
            Launched::Process { pid, placement } => {
                assert_eq!(placement, &Placement::Direct(DirectReason::NoSessionBus));
                *pid
            }
            Launched::Activated { .. } => panic!("activated over D-Bus"),
        })
        .collect()
}

#[test]
fn runs_exec_detached_in_path_with_the_context_environment() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work dir");
    std::fs::create_dir(&work).unwrap();
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=Recorder\nIcon=rec\n\
         Exec=\"{}\" --name=%c %i \"quoted arg\" %F\nPath={}\n",
        recorder(dir.path()).display(),
        work.display(),
    );
    let app = entry(dir.path(), "recorder.desktop", &text);
    let context = context(dir.path())
        .with_activation_token("token-1")
        .with_env("DESKRINCH_TEST_EXTRA", "extra value");
    let launched = launch(
        &app,
        None,
        &[Target::Path("/a b".into()), Target::Uri("file:///c".into())],
        &context,
    )
    .unwrap();
    let [pid] = pids(&launched)[..] else {
        panic!("one process expected");
    };
    let record = recorded(dir.path(), pid);
    assert_eq!(
        record["arg"],
        [
            "--name=Recorder",
            "--icon",
            "rec",
            "quoted arg",
            "/a b",
            "/c"
        ]
    );
    assert_eq!(record["cwd"], [work.display().to_string()]);
    assert_eq!(record["token"], ["token-1"]);
    assert_eq!(record["startup"], ["unset"]);
    assert_eq!(record["extra"], ["extra value"]);
    // Detached: not our child, and not in our session.
    assert_ne!(record["ppid"], [std::process::id().to_string()]);
    assert_ne!(record["sid"], [own_session().to_string()]);
}

#[test]
fn a_single_file_code_starts_one_process_per_target() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=R\nExec={} %u\n",
        recorder(dir.path()).display()
    );
    let app = entry(dir.path(), "r.desktop", &text);
    let targets = [
        Target::Uri("https://example.org".into()),
        Target::Path("/x".into()),
    ];
    let launched = launch(&app, None, &targets, &context(dir.path())).unwrap();
    let args: Vec<_> = pids(&launched)
        .into_iter()
        .map(|pid| recorded(dir.path(), pid)["arg"].clone())
        .collect();
    assert_eq!(args, [["https://example.org"], ["file:///x"]]);
}

#[test]
fn runs_actions() {
    let dir = tempfile::tempdir().unwrap();
    let recorder = recorder(dir.path());
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=R\nExec={0} main\nActions=new;\n\
         [Desktop Action new]\nName=New\nExec={0} --new-window\n",
        recorder.display()
    );
    let app = entry(dir.path(), "r.desktop", &text);
    let launched = launch(&app, Some("new"), &[], &context(dir.path())).unwrap();
    assert_eq!(
        recorded(dir.path(), pids(&launched)[0])["arg"],
        ["--new-window"]
    );
    let error = launch(&app, Some("nope"), &[], &context(dir.path())).unwrap_err();
    assert!(matches!(error, LaunchError::UnknownAction(id) if id == "nope"));
}

#[test]
fn a_terminal_entry_needs_a_terminal_and_runs_in_it() {
    let dir = tempfile::tempdir().unwrap();
    let recorder = recorder(dir.path());
    let text = "[Desktop Entry]\nType=Application\nName=R\nExec=top -d 1\nTerminal=true\n";
    let app = entry(dir.path(), "r.desktop", text);
    let error = launch(&app, None, &[], &context(dir.path())).unwrap_err();
    assert!(matches!(error, LaunchError::TerminalRequired));

    // The recorder stands in for the terminal emulator, so `top` never runs.
    let with_terminal = context(dir.path()).with_terminal([recorder.as_os_str(), "-e".as_ref()]);
    let launched = launch(&app, None, &[], &with_terminal).unwrap();
    assert_eq!(
        recorded(dir.path(), pids(&launched)[0])["arg"],
        ["-e", "top", "-d", "1"]
    );
}

#[test]
fn reports_what_cannot_be_launched() {
    let dir = tempfile::tempdir().unwrap();
    let link = entry(
        dir.path(),
        "l.desktop",
        "[Desktop Entry]\nType=Link\nName=L\nURL=https://example.org\n",
    );
    let context = context(dir.path());
    assert!(matches!(
        launch(&link, None, &[], &context),
        Err(LaunchError::NotAnApplication)
    ));
    let missing = entry(
        dir.path(),
        "m.desktop",
        "[Desktop Entry]\nType=Application\nName=M\nExec=/nonexistent/program\n",
    );
    assert!(matches!(
        launch(&missing, None, &[], &context),
        Err(LaunchError::Spawn { program, .. }) if program == "/nonexistent/program"
    ));
    let invalid = entry(
        dir.path(),
        "i.desktop",
        "[Desktop Entry]\nType=Application\nName=I\nExec=app %z\n",
    );
    assert!(matches!(
        launch(&invalid, None, &[], &context),
        Err(LaunchError::Exec(_))
    ));
    let no_dir = entry(
        dir.path(),
        "p.desktop",
        "[Desktop Entry]\nType=Application\nName=P\nExec=true\nPath=/nonexistent/dir\n",
    );
    assert!(matches!(
        launch(&no_dir, None, &[], &context),
        Err(LaunchError::Spawn { .. })
    ));
}

#[test]
fn a_dbus_activatable_entry_falls_back_to_exec_without_a_bus() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[Desktop Entry]\nType=Application\nName=R\nDBusActivatable=true\nExec={} fallback\n",
        recorder(dir.path()).display()
    );
    let app = entry(dir.path(), "org.deskrinch.Test.desktop", &text);
    let launched = launch(&app, None, &[], &context(dir.path())).unwrap();
    assert_eq!(
        recorded(dir.path(), pids(&launched)[0])["arg"],
        ["fallback"]
    );

    let no_exec = entry(
        dir.path(),
        "org.deskrinch.NoExec.desktop",
        "[Desktop Entry]\nType=Application\nName=N\nDBusActivatable=true\n",
    );
    assert!(matches!(
        launch(&no_exec, None, &[], &context(dir.path())),
        Err(LaunchError::NoCommand)
    ));
}

/// This process's session ID.
fn own_session() -> u32 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let after_comm = &stat[stat.rfind(')').unwrap() + 2..];
    after_comm.split(' ').nth(3).unwrap().parse().unwrap()
}
