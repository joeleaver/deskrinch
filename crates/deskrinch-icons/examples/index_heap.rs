//! Builds one index of this machine's real themes and keeps it to the end:
//! the smallest program to measure the index's heap and the cost of
//! building it under Valgrind. Build it with `cargo build --release -p
//! deskrinch-icons --example index_heap`, copy
//! `target/release/examples/index_heap` into a directory of your own
//! (`mktemp -d`), and run there:
//!
//! - The heap: `valgrind --tool=dhat --dhat-out-file=dhat.json
//!   ./index_heap [THEME]` (`breeze` by default). "At t-end" is the index
//!   (with this program's few bytes); "Total" counts every allocation made,
//!   nearly all of them while building. `dh_view.html` (with Valgrind)
//!   shows each allocation site of `dhat.json`.
//! - The instructions: `valgrind --tool=callgrind --dump-instr=no
//!   --separate-threads=no --callgrind-out-file=cg.out ./index_heap`, then
//!   `callgrind_annotate --inclusive=yes cg.out`: the line for
//!   `Index::load` is the whole build; `Lister::list` the listing,
//!   `TableBuilder::push` (inside it) the interning, and
//!   `TableBuilder::finish` the two sorts.
//!
//! It only reads: the base directories of this process's environment, as
//! `SearchPath::new` finds them.

use deskrinch_icons::{Index, SearchPath};
use deskrinch_xdg::BaseDirs;

fn main() {
    let theme = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "breeze".to_owned());
    let search_path = SearchPath::new(&BaseDirs::from_process());
    let index = Index::load(&search_path, &theme);
    println!(
        "theme {theme}: {} icon files, {} problems, consulting {:?}",
        index.files(),
        index.problems().len(),
        index.consulted()
    );
    // Leave without dropping it, so a heap profile's end ("At t-end") is
    // the index, as it is when a component holds it.
    std::process::exit(i32::from(index.files() == 0));
}
