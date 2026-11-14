//! Builds one index of this machine's real themes and keeps it to the end:
//! the smallest program to measure the index's heap and the cost of
//! building it under Valgrind. `cargo build --release -p deskrinch-icons
//! --example index_heap`, then for the heap `valgrind --tool=dhat
//! target/release/examples/index_heap [THEME]` (`breeze` by default): what
//! is live at the end is the index. For instructions, Callgrind's inclusive
//! count of `Index::load`.
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
