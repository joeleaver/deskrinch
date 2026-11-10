//! Watching directory trees.

use std::fs;

use deskrinch_watch::Target;

use crate::support::{Root, changed, quiet, watch, write};

#[test]
fn reports_changes_anywhere_in_a_tree() {
    let root = Root::new();
    let tree = root.join("share/applications");
    fs::create_dir_all(&tree).unwrap();
    let (_watcher, events) = watch(vec![Target::Tree(tree.clone())]);
    write(&tree.join("foot.desktop"), "a");
    changed(&events);
    // A subdirectory created after the watch began.
    write(&tree.join("kde/org.kde.konsole.desktop"), "b");
    changed(&events);
    write(&tree.join("kde/org.kde.konsole.desktop"), "c");
    changed(&events);
    // Beside the tree is not in it.
    write(&root.join("share/icons/x.png"), "d");
    quiet(&events);
}

#[test]
fn reports_a_tree_created_and_removed() {
    let root = Root::new();
    let tree = root.join("home/.local/share/applications");
    let (_watcher, events) = watch(vec![Target::Tree(tree.clone())]);
    write(&tree.join("foot.desktop"), "a");
    changed(&events);
    write(&tree.join("foot.desktop"), "b");
    changed(&events);
    fs::remove_dir_all(&tree).unwrap();
    changed(&events);
}
