//! A FIFO for a stand-in to be asked through, for the measure tests: a
//! stand-in reads it blocked, as an idle program waits, instead of polling
//! a file.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Makes a FIFO named `name` in `dir`; its path.
pub(crate) fn fifo(dir: &Path, name: &str) -> PathBuf {
    let fifo = dir.join(name);
    let made = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(made.success(), "mkfifo: {made}");
    fifo
}
