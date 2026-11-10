//! Scanning data directories on disk.

use std::os::unix::fs::symlink;

use deskrinch_desktop_entries::{
    Catalog, DesktopId, Deviation, ParseError, ProblemKind, ScanError, Visibility,
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
    let name = |i: &str| catalog.get(&id(i)).unwrap().entry.name.default.clone();
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
    let name = |i: &str| catalog.get(&id(i)).unwrap().entry.name.default.clone();
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
                format!("skipped: {:?}", ParseError::InvalidLine { line: 3 })
            ),
            (
                binary,
                format!("warning: {:?}", Deviation::NotUtf8 { line: 4 })
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
