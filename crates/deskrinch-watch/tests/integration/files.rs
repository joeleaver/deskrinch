//! Watching files: changes, creation, removal, renames and links.

use std::fs;
use std::os::unix::fs::symlink;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use deskrinch_watch::{Target, Timing, Watcher};

use crate::support::{Root, changed, quiet, watch, write};

#[test]
fn reports_a_change_to_a_file_once_it_settles() {
    let root = Root::new();
    let file = root.join("config.toml");
    write(&file, "a");
    let (_watcher, events) = watch(vec![Target::File(file.clone())]);
    // A burst is one report.
    for text in ["b", "c", "d"] {
        write(&file, text);
    }
    changed(&events);
    quiet(&events);
}

#[test]
fn reports_creation_through_missing_directories_and_removal() {
    let root = Root::new();
    let file = root.join("home/.config/deskrinch/config.toml");
    let (_watcher, events) = watch(vec![Target::File(file.clone())]);
    write(&file, "a");
    changed(&events);
    // Now watched in its own directory.
    write(&file, "b");
    changed(&events);
    fs::remove_file(&file).unwrap();
    changed(&events);
    fs::remove_dir_all(root.join("home")).unwrap();
    changed(&events);
    write(&file, "c");
    changed(&events);
}

#[test]
fn reports_a_save_by_rename() {
    let root = Root::new();
    let file = root.join("config.toml");
    write(&file, "a");
    let (_watcher, events) = watch(vec![Target::File(file.clone())]);
    let temporary = root.join(".config.toml.tmp");
    fs::write(&temporary, "b").unwrap();
    fs::rename(&temporary, &file).unwrap();
    changed(&events);
}

#[test]
fn ignores_other_files_in_the_directory() {
    let root = Root::new();
    let file = root.join("config.toml");
    write(&file, "a");
    let (_watcher, events) = watch(vec![Target::File(file)]);
    write(&root.join("notes.txt"), "unrelated");
    assert_eq!(fs::read_to_string(root.join("config.toml")).unwrap(), "a");
    quiet(&events);
}

#[test]
fn reports_a_change_when_asked_to_refresh() {
    let root = Root::new();
    let (watcher, events) = watch(vec![Target::File(root.join("config.toml"))]);
    watcher.refresh();
    changed(&events);
    quiet(&events);
}

#[test]
fn may_be_dropped_by_its_own_subscriber() {
    let root = Root::new();
    let file = root.join("config.toml");
    let slot: Arc<Mutex<Option<Watcher>>> = Arc::default();
    let (done, finished) = mpsc::channel();
    let inner = Arc::clone(&slot);
    let watcher = Watcher::start(
        vec![Target::File(file.clone())],
        Timing::default(),
        "test-watch",
        move |_| {
            // The last owner, dropped on the watcher's own thread.
            drop(inner.lock().unwrap().take());
            done.send(()).ok();
        },
    )
    .unwrap();
    *slot.lock().unwrap() = Some(watcher);
    write(&file, "a");
    finished.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(slot.lock().unwrap().is_none());
}

#[test]
fn follows_a_link_and_sees_it_re_pointed() {
    let root = Root::new();
    let first = root.join("dotfiles/first.toml");
    let second = root.join("elsewhere/second.toml");
    write(&first, "a");
    write(&second, "b");
    let file = root.join("config/config.toml");
    fs::create_dir_all(root.join("config")).unwrap();
    symlink(&first, &file).unwrap();
    let (_watcher, events) = watch(vec![Target::File(file.clone())]);

    // Editing the target, in a directory of its own.
    write(&first, "a2");
    changed(&events);

    // Re-pointing the link, as `ln -sfn` does: a new link renamed over it.
    let replacement = root.join("config/.config.toml.new");
    symlink(&second, &replacement).unwrap();
    fs::rename(&replacement, &file).unwrap();
    changed(&events);

    // Now the new target is watched, and the old one no longer concerns it.
    write(&second, "b2");
    changed(&events);
    write(&first, "a3");
    quiet(&events);
}
