//! Watching data directories for changes.

use std::fs;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use deskrinch_desktop_entries::{Event, Visibility, Watcher};

use crate::support::{PATIENCE, environment, script, write};

fn app(name: &str, extra: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run\n{extra}")
}

/// The next event, as `(kind, id, name)`.
fn next(events: &Receiver<Event>) -> (String, String, String) {
    match events.recv_timeout(PATIENCE).expect("no event arrived") {
        Event::Added(e) => ("added".into(), e.entry.id.to_string(), e.entry.name.default),
        Event::Changed(e) => (
            "changed".into(),
            e.entry.id.to_string(),
            e.entry.name.default,
        ),
        Event::Removed(id) => ("removed".into(), id.to_string(), String::new()),
        Event::Error(error) => panic!("watch error: {error}"),
    }
}

fn expect(events: &Receiver<Event>, kind: &str, id: &str, name: &str) {
    assert_eq!(next(events), (kind.into(), id.into(), name.into()));
}

#[test]
fn reports_entries_added_changed_and_removed() {
    let root = tempfile::tempdir().unwrap();
    // The user's directory does not exist yet; the system one does.
    let home = root.path().join("home/.local/share");
    let system = root.path().join("system");
    write(&system, "applications/b.desktop", &app("B", ""));
    let (sender, events) = mpsc::channel();
    let (watcher, catalog) = Watcher::start(environment(&home, &[&system]), move |event| {
        sender.send(event).unwrap();
    })
    .unwrap();
    let ids: Vec<_> = catalog.iter().map(|e| e.entry.id.to_string()).collect();
    assert_eq!(ids, ["b.desktop"]);

    // Creating the missing directory with an entry in it.
    let a = write(&home, "applications/a.desktop", &app("A", ""));
    expect(&events, "added", "a.desktop", "A");

    write(&home, "applications/a.desktop", &app("A2", ""));
    expect(&events, "changed", "a.desktop", "A2");

    // A user override hiding a system entry, then its removal.
    let hide = write(&home, "applications/b.desktop", &app("B", "Hidden=true\n"));
    expect(&events, "removed", "b.desktop", "");
    fs::remove_file(&hide).unwrap();
    expect(&events, "added", "b.desktop", "B");

    // An entry in a new subdirectory.
    write(&home, "applications/vendor/c.desktop", &app("C", ""));
    expect(&events, "added", "vendor-c.desktop", "C");

    // The visibility counts as a change.
    write(
        &system,
        "applications/b.desktop",
        &app("B", "NoDisplay=true\n"),
    );
    expect(&events, "changed", "b.desktop", "B");

    fs::remove_file(&a).unwrap();
    expect(&events, "removed", "a.desktop", "");

    // Removing the whole directory, then recreating it.
    fs::remove_dir_all(home.join("applications")).unwrap();
    expect(&events, "removed", "vendor-c.desktop", "");
    write(&home, "applications/d.desktop", &app("D", ""));
    expect(&events, "added", "d.desktop", "D");

    drop(watcher);
    assert!(
        events.recv().is_err(),
        "the watcher stopped and dropped its subscriber"
    );
}

#[test]
fn refresh_re_evaluates_try_exec_and_exec() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    fs::create_dir_all(&bin).unwrap();
    write(
        &home,
        "applications/t.desktop",
        &app("T", "TryExec=late-tool\n"),
    );
    let mut env = environment(&home, &[]);
    env.path = vec![bin.clone()];
    let (sender, events) = mpsc::channel();
    let (watcher, catalog) = Watcher::start(env, move |event| {
        sender.send(event).unwrap();
    })
    .unwrap();
    let first = catalog.iter().next().unwrap();
    assert_eq!(first.visibility, Visibility::TryExecMissing);

    // Installing a program touches no watched directory: nothing is
    // reported until a refresh. With `TryExec` found, the entry's `Exec`
    // program (`run`) is still missing; installing it shows the entry.
    for (program, visibility) in [
        ("late-tool", Visibility::ExecMissing),
        ("run", Visibility::Shown),
    ] {
        script(&bin.join(program), "true\n");
        assert!(events.recv_timeout(Duration::from_millis(500)).is_err());
        watcher.refresh();
        match events.recv_timeout(PATIENCE).expect("no event arrived") {
            Event::Changed(e) => assert_eq!(e.visibility, visibility, "{program}"),
            other => panic!("unexpected event {other:?}"),
        }
    }
    // A refresh that finds nothing new reports nothing.
    watcher.refresh();
    assert!(events.recv_timeout(Duration::from_millis(500)).is_err());
}
