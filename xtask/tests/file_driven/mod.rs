//! A stand-in driven by files, for the measure tests of commands: an `sh`
//! script that prints its first lines, then, each time a file it watches
//! appears, removes it and does what that file asks; and the command that
//! makes the file.

use std::fmt::Write as _;
use std::path::Path;

/// A main-window frame line, as rinch writes it.
pub(crate) const MAIN_FRAME: &str = "'[PERF] frame 1: style 0.10ms'";

/// A stand-in that prints `start` (its first lines), then, each time
/// `dir/go-N` appears, removes it and runs the Nth of `on_go`, a shell
/// snippet (`printf '%s\n' LINES >&2` prints lines as the program would).
pub(crate) fn stand_in(dir: &Path, start: &str, on_go: &[&str]) -> String {
    let dir = dir.display();
    let mut watch = String::new();
    for (index, action) in on_go.iter().enumerate() {
        let n = index + 1;
        write!(
            watch,
            "if [ -e {dir}/go-{n} ]; then rm {dir}/go-{n}; {action}; fi; "
        )
        .unwrap();
    }
    format!("printf '%s\\n' {start} >&2; while :; do {watch}sleep 0.05; done")
}

/// The snippet that prints `lines` (each quoted for `sh`) as the program's
/// output, for [`stand_in`]'s `on_go`.
pub(crate) fn prints(lines: &str) -> String {
    format!("printf '%s\\n' {lines} >&2")
}

/// The command that makes `dir/go-n`.
pub(crate) fn go(dir: &Path, n: usize) -> String {
    format!("touch {}/go-{n}", dir.display())
}
