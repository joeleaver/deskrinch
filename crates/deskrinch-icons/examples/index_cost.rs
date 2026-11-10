//! What building an index costs for this machine's real themes, in CPU
//! time: `cargo run --release -p deskrinch-icons --example index_cost --
//! [THEME] [BUILDS]` (`breeze` and 20 by default).
//!
//! It only reads: the base directories of this process's environment, as
//! `SearchPath::new` finds them. CPU time is this thread's own
//! (`CLOCK_THREAD_CPUTIME_ID`), which leaves out time spent waiting for a
//! CPU, so the figures hold on a loaded machine where wall time does not.
//! The first build reads directories the kernel may not have cached; the
//! others show the cost with a warm cache, as a rebuild after a change has.

use std::time::Duration;

use deskrinch_icons::{Index, Request, SearchPath};
use deskrinch_xdg::BaseDirs;
use rustix::time::{ClockId, clock_gettime};

fn cpu_time() -> Duration {
    let now = clock_gettime(ClockId::ThreadCPUTime);
    let seconds = u64::try_from(now.tv_sec).unwrap_or(0);
    let nanos = u32::try_from(now.tv_nsec).unwrap_or(0);
    Duration::new(seconds, nanos)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let theme = args.next().unwrap_or_else(|| "breeze".to_owned());
    let builds: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(20);
    let search_path = SearchPath::new(&BaseDirs::from_process());

    let mut costs = Vec::with_capacity(builds);
    let mut index = None;
    for _ in 0..builds.max(1) {
        let start = cpu_time();
        let built = Index::load(&search_path, &theme);
        costs.push(cpu_time().saturating_sub(start));
        index = Some(built);
    }
    let Some(index) = index else { return };
    let first = costs[0];
    let mut warm = costs[1..].to_vec();
    warm.sort();

    println!("theme {theme}: consults {:?}", index.consulted());
    let installed = index
        .consulted()
        .iter()
        .filter_map(|name| index.installed(name));
    let directories: usize = installed.map(|theme| theme.directories.len()).sum();
    println!(
        "{} icon files in {directories} theme directories, {} problems",
        index.files(),
        index.problems().len()
    );
    println!("first build: {first:?} CPU");
    if let (Some(min), Some(median)) = (warm.first(), warm.get(warm.len() / 2)) {
        println!(
            "warm builds ({}): min {min:?}, median {median:?} CPU",
            warm.len()
        );
    }

    let names = [
        "document-open",
        "firefox",
        "folder",
        "no-such-icon-anywhere",
    ];
    let lookups = 100_000;
    let start = cpu_time();
    let mut hits = 0;
    for i in 0..lookups {
        let request = Request::new(names[i % names.len()], 48).with_generic_fallback();
        hits += usize::from(index.lookup(&request).is_ok());
    }
    let per_lookup = cpu_time().saturating_sub(start) / lookups.try_into().unwrap_or(u32::MAX);
    println!("lookup: {per_lookup:?} CPU each ({hits} of {lookups} found)");
}
