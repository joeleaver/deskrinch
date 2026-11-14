//! Scanning data directories on disk.

use std::os::unix::fs::symlink;

use deskrinch_desktop_entries::{
    Catalog, DesktopId, Deviation, KeyFileDeviation, ParseError, ProblemKind, ScanError,
    SyntaxError, Visibility,
};

use crate::support::{environment, script, write};

fn app(name: &str, extra: &str) -> String {
    format!("[Desktop Entry]\nType=Application\nName={name}\nExec=run\n{extra}")
}

fn id(text: &str) -> DesktopId {
    DesktopId::new(text).unwrap()
}

#[test]
fn applies_precedence_ids_and_hidden_across_directories() {
    let root = tempfile::tempdir().unwrap();
    let (home, high, low) = (
        root.path().join("home"),
        root.path().join("high"),
        root.path().join("low"),
    );
    write(
        &home,
        "applications/editor.desktop",
        &app("Home Editor", ""),
    );
    write(
        &high,
        "applications/editor.desktop",
        &app("High Editor", ""),
    );
    write(
        &high,
        "applications/kde/konsole.desktop",
        &app("Konsole", ""),
    );
    write(
        &low,
        "applications/kde-konsole.desktop",
        &app("Low Konsole", ""),
    );
    write(&low, "applications/gone.desktop", &app("Gone", ""));
    write(
        &home,
        "applications/gone.desktop",
        &app("Gone", "Hidden=true\n"),
    );
    write(&low, "applications/readme.txt", "not an entry");
    let catalog = Catalog::scan(&environment(&home, &[&high, &low]));

    let ids: Vec<_> = catalog.iter().map(|e| e.entry.id.to_string()).collect();
    assert_eq!(ids, ["editor.desktop", "kde-konsole.desktop"]);
    let name = |i: &str| catalog.get(&id(i)).unwrap().entry.name.value.clone();
    assert_eq!(name("editor.desktop"), "Home Editor");
    assert_eq!(name("kde-konsole.desktop"), "Konsole");
    assert!(catalog.problems().is_empty());
}

#[test]
fn skips_unusable_files_and_warns_about_deviant_ones() {
    let root = tempfile::tempdir().unwrap();
    let (home, system) = (root.path().join("home"), root.path().join("system"));
    let broken = write(
        &home,
        "applications/a.desktop",
        "[Desktop Entry]\nType=Application\nnot a key\n",
    );
    write(&system, "applications/a.desktop", &app("System A", ""));
    let binary = system.join("applications/binary.desktop");
    std::fs::write(
        &binary,
        b"[Desktop Entry]\nType=Application\nName=B\nComment=\xff\nExec=b\n",
    )
    .unwrap();
    let catalog = Catalog::scan(&environment(&home, &[&system]));

    // The unusable override falls through to the system entry; the
    // deviant file is listed, with a warning.
    let name = |i: &str| catalog.get(&id(i)).unwrap().entry.name.value.clone();
    assert_eq!(name("a.desktop"), "System A");
    assert_eq!(name("binary.desktop"), "B");
    let problems: Vec<_> = catalog
        .problems()
        .iter()
        .map(|p| match &p.kind {
            ProblemKind::Skipped(ScanError::Parse(error)) => {
                (p.path.clone(), format!("skipped: {error:?}"))
            }
            ProblemKind::Skipped(ScanError::Read(error)) => panic!("read error {error}"),
            ProblemKind::Deviation(deviation) => {
                (p.path.clone(), format!("warning: {deviation:?}"))
            }
        })
        .collect();
    assert_eq!(
        problems,
        [
            (
                broken,
                format!(
                    "skipped: {:?}",
                    ParseError::Syntax(SyntaxError::InvalidLine { line: 3 })
                )
            ),
            (
                binary,
                format!(
                    "warning: {:?}",
                    Deviation::KeyFile(KeyFileDeviation::NotUtf8 { line: 4 })
                )
            ),
        ]
    );
    assert!(catalog.problems()[0].is_skipped() && !catalog.problems()[1].is_skipped());
}

#[test]
fn checks_try_exec_against_path_and_absolute_paths() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin) = (root.path().join("home"), root.path().join("bin"));
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin.join("present"), "true\n");
    // The program every entry here runs.
    script(&bin.join("run"), "true\n");
    std::fs::write(bin.join("not-executable"), "").unwrap();
    let absolute = bin.join("present");
    write(
        &home,
        "applications/onpath.desktop",
        &app("A", "TryExec=present\n"),
    );
    write(
        &home,
        "applications/absolute.desktop",
        &app("B", &format!("TryExec={}\n", absolute.display())),
    );
    write(
        &home,
        "applications/missing.desktop",
        &app("C", "TryExec=absent\n"),
    );
    write(
        &home,
        "applications/noexec.desktop",
        &app("D", "TryExec=not-executable\n"),
    );
    let mut env = environment(&home, &[]);
    env.path = vec![bin];
    let catalog = Catalog::scan(&env);

    let visibility = |i: &str| catalog.get(&id(i)).unwrap().visibility;
    assert_eq!(visibility("onpath.desktop"), Visibility::Shown);
    assert_eq!(visibility("absolute.desktop"), Visibility::Shown);
    assert_eq!(visibility("missing.desktop"), Visibility::TryExecMissing);
    assert_eq!(visibility("noexec.desktop"), Visibility::TryExecMissing);
}

#[test]
fn hides_applications_whose_exec_program_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let (home, bin, work) = (
        root.path().join("home"),
        root.path().join("bin"),
        root.path().join("work"),
    );
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(work.join("sub")).unwrap();
    script(&bin.join("on-path"), "true\n");
    script(&work.join("sub/relative"), "true\n");
    std::fs::write(bin.join("not-executable"), "").unwrap();
    let absolute = bin.join("on-path");
    let entries = [
        ("path", "Exec=on-path %U\n".to_owned()),
        ("absolute", format!("Exec=\"{}\"\n", absolute.display())),
        (
            "relative",
            format!("Exec=sub/relative\nPath={}\n", work.display()),
        ),
        ("uninstalled", "Exec=uninstalled-app\n".to_owned()),
        ("not-executable", "Exec=not-executable\n".to_owned()),
        // A bare name is looked up in PATH, never in `Path`.
        (
            "bare-in-path-dir",
            format!("Exec=relative\nPath={}\n", work.join("sub").display()),
        ),
        (
            "dbus",
            "DBusActivatable=true\nExec=uninstalled-app\n".to_owned(),
        ),
    ];
    for (id, body) in &entries {
        let text = format!("[Desktop Entry]\nType=Application\nName={id}\n{body}");
        write(&home, &format!("applications/{id}.desktop"), &text);
    }
    let mut env = environment(&home, &[]);
    env.path = vec![bin];
    let catalog = Catalog::scan(&env);

    let visibility = |i: &str| {
        catalog
            .get(&id(&format!("{i}.desktop")))
            .unwrap()
            .visibility
    };
    assert_eq!(visibility("path"), Visibility::Shown);
    assert_eq!(visibility("absolute"), Visibility::Shown);
    assert_eq!(visibility("relative"), Visibility::Shown);
    assert_eq!(visibility("uninstalled"), Visibility::ExecMissing);
    assert_eq!(visibility("not-executable"), Visibility::ExecMissing);
    assert_eq!(visibility("bare-in-path-dir"), Visibility::ExecMissing);
    assert_eq!(visibility("dbus"), Visibility::Shown);
    let shown: Vec<_> = catalog.shown().map(|e| e.id.to_string()).collect();
    assert_eq!(
        shown,
        [
            "absolute.desktop",
            "dbus.desktop",
            "path.desktop",
            "relative.desktop"
        ]
    );
    // Tools still see the hidden entries, and why.
    assert_eq!(catalog.len(), entries.len());
}

#[test]
fn follows_symbolic_links_without_looping() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let elsewhere = root.path().join("elsewhere");
    write(&elsewhere, "linked.desktop", &app("Linked", ""));
    write(&home, "applications/sub/a.desktop", &app("A", ""));
    symlink(&elsewhere, home.join("applications/vendor")).unwrap();
    symlink(
        home.join("applications"),
        home.join("applications/sub/loop"),
    )
    .unwrap();
    let catalog = Catalog::scan(&environment(&home, &[]));

    let ids: Vec<_> = catalog.iter().map(|e| e.entry.id.to_string()).collect();
    assert_eq!(ids, ["sub-a.desktop", "vendor-linked.desktop"]);
}

#[test]
fn a_missing_directory_contributes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::scan(&environment(&root.path().join("nowhere"), &[]));
    assert!(catalog.is_empty() && catalog.problems().is_empty());
}
