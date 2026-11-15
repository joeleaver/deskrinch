//! How a file written in place is reported: once its writer closes it,
//! never while it is open (the watcher asks the kernel for `CLOSE_WRITE`,
//! not `MODIFY`).

use std::fs::OpenOptions;
use std::io::Write as _;

use deskrinch_watch::Target;

use crate::support::{Root, changed, quiet, watch, write};

#[test]
fn a_file_rewritten_in_place_is_reported_once_it_is_closed_not_while_open() {
    let root = Root::new();
    let file = root.join("config.toml");
    write(&file, "[panel]\nheight = 40\n");
    let (_watcher, events) = watch(vec![Target::File(file.clone())]);
    // Truncated and half written, through a descriptor the test holds open.
    let mut open = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&file)
        .unwrap();
    open.write_all(b"[panel]\n").unwrap();
    open.flush().unwrap();
    // Nothing while it stays open: a quiet window (see `quiet`), which a
    // slow machine can only make longer, not fail.
    quiet(&events);
    open.write_all(b"height = 50\n").unwrap();
    drop(open);
    // Closed: reported.
    changed(&events);
}
