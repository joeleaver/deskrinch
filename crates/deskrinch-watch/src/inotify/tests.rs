//! The backend against the real kernel, in a temporary directory: what
//! wakes it, and what does not.

use super::*;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};

const TREE: How = How {
    recursive: true,
    entries: true,
};
const NAMES_ONLY: How = How {
    recursive: false,
    entries: false,
};

/// Long enough for an event already queued to be read; nothing here waits
/// for one that is not.
const SETTLE: Duration = Duration::from_millis(200);

fn root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().canonicalize().unwrap();
    (dir, path)
}

/// The paths the next wait reads, or none when it reads nothing.
fn changed(inotify: &mut Inotify) -> Vec<PathBuf> {
    match inotify.wait(Some(SETTLE)) {
        Wait::Changes(changes) => changes
            .into_iter()
            .map(|change| match change {
                Change::At(path) => path,
                other => panic!("{other:?}"),
            })
            .collect(),
        Wait::Quiet => Vec::new(),
        other => panic!("{other:?}"),
    }
}

/// A tree with a file and a subdirectory with a file, watched.
fn watched_tree() -> (tempfile::TempDir, PathBuf, Inotify) {
    let (dir, root) = root();
    fs::create_dir_all(root.join("tree/sub")).unwrap();
    fs::write(root.join("tree/a.desktop"), "a").unwrap();
    fs::write(root.join("tree/sub/b.desktop"), "b").unwrap();
    let mut inotify = Inotify::new().unwrap();
    inotify.watch(&root.join("tree"), TREE).unwrap();
    (dir, root, inotify)
}

#[test]
fn reading_and_listing_a_watched_tree_reads_no_event() {
    let (_dir, root, mut inotify) = watched_tree();
    for _ in 0..3 {
        let mut text = String::new();
        File::open(root.join("tree/a.desktop"))
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        fs::read_to_string(root.join("tree/sub/b.desktop")).unwrap();
        fs::read_dir(root.join("tree")).unwrap().for_each(drop);
        fs::read_dir(root.join("tree/sub")).unwrap().for_each(drop);
        fs::metadata(root.join("tree/a.desktop")).unwrap();
    }
    assert!(
        matches!(inotify.wait(Some(SETTLE)), Wait::Quiet),
        "reads woke the watch"
    );
}

#[test]
fn opening_files_in_a_directory_watched_for_a_name_reads_no_event() {
    let (_dir, root) = root();
    fs::create_dir_all(root.join("share/applications")).unwrap();
    fs::write(root.join("share/mime.cache"), "x").unwrap();
    let mut inotify = Inotify::new().unwrap();
    inotify.watch(&root.join("share"), NAMES_ONLY).unwrap();
    fs::read_to_string(root.join("share/mime.cache")).unwrap();
    fs::read_dir(root.join("share/applications"))
        .unwrap()
        .for_each(drop);
    // Written in place: a name already there, which a name's watch does not
    // follow.
    fs::write(root.join("share/mime.cache"), "y").unwrap();
    assert!(matches!(inotify.wait(Some(SETTLE)), Wait::Quiet));
}

#[test]
fn each_kind_of_change_in_a_tree_is_read() {
    let (_dir, root, mut inotify) = watched_tree();
    let tree = root.join("tree");

    fs::write(tree.join("new.desktop"), "n").unwrap();
    assert!(
        changed(&mut inotify).contains(&tree.join("new.desktop")),
        "made and written"
    );

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(tree.join("a.desktop"))
        .unwrap();
    file.write_all(b"more").unwrap();
    drop(file);
    assert!(
        changed(&mut inotify).contains(&tree.join("a.desktop")),
        "written and closed"
    );

    fs::set_permissions(tree.join("a.desktop"), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        changed(&mut inotify).contains(&tree.join("a.desktop")),
        "attributes"
    );

    fs::rename(tree.join("a.desktop"), root.join("out.desktop")).unwrap();
    assert!(
        changed(&mut inotify).contains(&tree.join("a.desktop")),
        "moved out"
    );

    fs::write(root.join("in.desktop"), "i").unwrap();
    fs::rename(root.join("in.desktop"), tree.join("sub/in.desktop")).unwrap();
    assert!(
        changed(&mut inotify).contains(&tree.join("sub/in.desktop")),
        "moved in, below"
    );

    fs::remove_file(tree.join("sub/b.desktop")).unwrap();
    assert!(
        changed(&mut inotify).contains(&tree.join("sub/b.desktop")),
        "removed"
    );
}

#[test]
fn a_directory_made_in_a_tree_is_watched_from_its_event() {
    let (_dir, root, mut inotify) = watched_tree();
    let tree = root.join("tree");
    fs::create_dir(tree.join("kde")).unwrap();
    assert!(changed(&mut inotify).contains(&tree.join("kde")));
    fs::write(tree.join("kde/c.desktop"), "c").unwrap();
    assert!(changed(&mut inotify).contains(&tree.join("kde/c.desktop")));
}

#[test]
fn a_directory_watched_for_a_name_sees_names_made_and_removed() {
    let (_dir, root) = root();
    fs::create_dir_all(root.join("share")).unwrap();
    let mut inotify = Inotify::new().unwrap();
    inotify.watch(&root.join("share"), NAMES_ONLY).unwrap();
    fs::create_dir(root.join("share/applications")).unwrap();
    assert!(changed(&mut inotify).contains(&root.join("share/applications")));
    fs::remove_dir(root.join("share/applications")).unwrap();
    assert!(changed(&mut inotify).contains(&root.join("share/applications")));
}

#[test]
fn a_watched_directory_removed_is_a_change() {
    let (_dir, root, mut inotify) = watched_tree();
    fs::remove_dir_all(root.join("tree")).unwrap();
    match inotify.wait(Some(SETTLE)) {
        Wait::Changes(changes) => assert!(!changes.is_empty()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_names_of_one_directory_share_their_events() {
    let (_dir, root) = root();
    fs::create_dir_all(root.join("real")).unwrap();
    symlink(root.join("real"), root.join("link")).unwrap();
    let mut inotify = Inotify::new().unwrap();
    // The entries' contents through one name, names only through the
    // other: the descriptor must keep both.
    inotify.watch(&root.join("real"), TREE).unwrap();
    inotify.watch(&root.join("link"), NAMES_ONLY).unwrap();
    fs::write(root.join("real/f"), "x").unwrap();
    changed(&mut inotify);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(root.join("real/f"))
        .unwrap();
    file.write_all(b"y").unwrap();
    drop(file);
    assert!(
        changed(&mut inotify).contains(&root.join("real/f")),
        "the second name's add kept the first's events"
    );
    // The second name going leaves the first's.
    inotify.unwatch(&root.join("link"));
    fs::write(root.join("real/g"), "z").unwrap();
    assert!(changed(&mut inotify).contains(&root.join("real/g")));
}

#[test]
fn the_waker_wakes_a_wait() {
    let (_dir, root) = root();
    let mut inotify = Inotify::new().unwrap();
    inotify.watch(&root, NAMES_ONLY).unwrap();
    (inotify.waker())();
    assert!(matches!(
        inotify.wait(Some(Duration::from_secs(10))),
        Wait::Woken
    ));
    assert!(matches!(inotify.wait(Some(SETTLE)), Wait::Quiet), "once");
}

#[test]
fn a_link_back_up_a_tree_is_walked_once() {
    let (_dir, root) = root();
    fs::create_dir_all(root.join("tree/sub")).unwrap();
    symlink(root.join("tree"), root.join("tree/sub/up")).unwrap();
    let found = directories(&root.join("tree"));
    assert_eq!(found, [root.join("tree"), root.join("tree/sub")]);
}
