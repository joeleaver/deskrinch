use std::collections::BTreeMap;
use std::time::Duration;

use super::proc::{
    Children, Task, children_by_parent, descendants, parse_children, parse_schedstat, parse_stat,
};
use super::window::{Polled, Tally};
use super::{MAX_POLL_GAP, POLL, WINDOW};

#[test]
fn schedstat_gives_run_and_wait() {
    assert_eq!(
        parse_schedstat("2963372 612619 17\n"),
        Some((2_963_372, 612_619))
    );
    assert_eq!(parse_schedstat("2963372\n"), None);
    assert_eq!(parse_schedstat(""), None);
}

#[test]
fn stat_gives_state_and_parent_past_a_name_with_spaces_and_parentheses() {
    let stat = "4242 (a (b) c) R 17 4242 4242 0 -1 4194560 100 0 0 0 5 3";
    assert_eq!(parse_stat(stat), Some(('R', 17)));
    assert_eq!(parse_stat("4242 (sway) S 1 4242"), Some(('S', 1)));
    assert_eq!(parse_stat("4242 (sway)"), None);
}

#[test]
fn a_children_file_lists_process_ids() {
    assert_eq!(parse_children("1906618 1906619 \n"), [1_906_618, 1_906_619]);
    assert_eq!(parse_children(""), Vec::<u32>::new());
}

#[test]
fn descendants_are_found_at_any_depth_and_others_are_not() {
    // A listing: 1 -> 10, 20; 10 -> 11, 14; 11 -> 12 -> 13 (a deep child);
    // 20 -> 21, unrelated to 10. 12 is listed twice, as a listing taken
    // while processes come and go may list one.
    let listing: BTreeMap<u32, Vec<u32>> = [
        (1, vec![10, 20]),
        (10, vec![11, 14]),
        (11, vec![12, 12]),
        (12, vec![13]),
        (20, vec![21]),
    ]
    .into();
    let children = |pid| listing.get(&pid).cloned().unwrap_or_default();
    assert_eq!(descendants(children, 10), [11, 12, 13, 14].into());
    assert_eq!(descendants(children, 13), [].into());
    assert_eq!(descendants(children, 99), [].into());
}

#[test]
fn without_children_files_the_process_table_is_scanned_to_the_same_descendants() {
    // The way chosen: the files where the kernel has them, else the scan.
    assert_eq!(Children::of(true), Children::Files);
    assert_eq!(Children::of(false), Children::Scan);
    // The scan's listing (each process's parent) gives the descendants the
    // files' listing (each process's children) gives.
    let parents: BTreeMap<u32, u32> = [
        (10, 1),
        (20, 1),
        (11, 10),
        (14, 10),
        (12, 11),
        (13, 12),
        (21, 20),
    ]
    .into();
    let by_parent = children_by_parent(&parents);
    let scanned = descendants(|pid| by_parent.get(&pid).cloned().unwrap_or_default(), 10);
    assert_eq!(scanned, [11, 12, 13, 14].into());
}

fn task(run_ns: u64, wait_ns: u64, state: char) -> Task {
    Task {
        run_ns,
        wait_ns,
        state,
    }
}

/// One process, "p", with the threads given.
fn polled(threads: &[(u32, Task)]) -> Polled {
    vec![("p".to_owned(), threads.iter().copied().collect())]
}

/// The polls of a whole window, on time: the start, then ten polls of
/// `each(n)` a [`POLL`] apart.
fn window(start: &Polled, each: impl Fn(u32) -> Polled) -> Tally {
    let mut tally = Tally::new(start);
    for n in 1..=10 {
        tally.add(POLL * n, &each(n));
    }
    tally
}

const MS: u64 = 1_000_000;

#[test]
fn a_sleeping_process_is_quiet() {
    let start = polled(&[(1, task(50 * MS, 9 * MS, 'S'))]);
    let tally = window(&start, |_| polled(&[(1, task(50 * MS, 9 * MS, 'S'))]));
    assert!(tally.verdict(WINDOW).quiet());
}

#[test]
fn running_waiting_or_busy_each_makes_it_unquiet() {
    let start = polled(&[(1, task(0, 0, 'S'))]);
    // Ran 5 ms over the window: at the bound, not under it.
    let ran = window(&start, |n| {
        polled(&[(1, task(u64::from(n) * MS / 2, 0, 'S'))])
    });
    assert!(!ran.verdict(WINDOW).quiet());
    // Waited 100 ms to run: at the bound.
    let waited = window(&start, |n| {
        polled(&[(1, task(0, u64::from(n) * 10 * MS, 'S'))])
    });
    assert!(!waited.verdict(WINDOW).quiet());
    // Starved through the window: no run, no wait yet, but runnable.
    let starved = window(&start, |_| polled(&[(1, task(0, 0, 'R'))]));
    let verdict = starved.verdict(WINDOW);
    assert!(!verdict.quiet());
    assert_eq!(verdict.figures[0].busy_polls, 10);
    // Blocked on disk at one poll.
    let blocked = window(&start, |n| {
        polled(&[(1, task(0, 0, if n == 4 { 'D' } else { 'S' }))])
    });
    assert!(!blocked.verdict(WINDOW).quiet());
    // Under both bounds and never busy at a poll: a little work, and the
    // queueing a loaded machine gives its wakes.
    let little = window(&start, |_| polled(&[(1, task(4 * MS, 90 * MS, 'S'))]));
    assert!(little.verdict(WINDOW).quiet());
}

#[test]
fn a_thread_born_in_the_window_counts_all_it_did() {
    let start = polled(&[(1, task(0, 0, 'S'))]);
    let tally = window(&start, |n| {
        if n < 5 {
            polled(&[(1, task(0, 0, 'S'))])
        } else {
            polled(&[(1, task(0, 0, 'S')), (2, task(6 * MS, 0, 'S'))])
        }
    });
    let verdict = tally.verdict(WINDOW);
    assert_eq!(verdict.figures[0].run_ns, 6 * MS);
    assert!(!verdict.quiet());
}

#[test]
fn a_thread_that_ends_keeps_what_it_was_counted() {
    let start = polled(&[(1, task(0, 0, 'S')), (2, task(0, 0, 'S'))]);
    let tally = window(&start, |n| {
        if n < 5 {
            polled(&[(1, task(0, 0, 'S')), (2, task(6 * MS, 0, 'S'))])
        } else {
            polled(&[(1, task(0, 0, 'S'))])
        }
    });
    assert_eq!(tally.verdict(WINDOW).figures[0].run_ns, 6 * MS);
}

#[test]
fn a_process_first_seen_in_the_window_is_watched_from_then() {
    let start = polled(&[(1, task(0, 0, 'S'))]);
    let tally = window(&start, |n| {
        let mut polled = polled(&[(1, task(0, 0, 'S'))]);
        if n > 6 {
            polled.push(("child".to_owned(), [(7, task(0, 0, 'R'))].into()));
        }
        polled
    });
    let verdict = tally.verdict(WINDOW);
    let child = verdict.figures.iter().find(|f| f.label == "child").unwrap();
    assert_eq!(child.busy_polls, 4);
    assert!(!verdict.quiet());
}

#[test]
fn a_window_whose_polls_came_late_is_not_quiet() {
    let quiet = polled(&[(1, task(0, 0, 'S'))]);
    // Three polls in the whole window: the tool was starved.
    let mut few = Tally::new(&quiet);
    for at in [300, 600, 900] {
        few.add(Duration::from_millis(at), &quiet);
    }
    let verdict = few.verdict(WINDOW);
    assert!(!verdict.on_time);
    assert!(!verdict.quiet());
    // Enough polls, but one gap longer than the most allowed.
    let mut gap = Tally::new(&quiet);
    let late = MAX_POLL_GAP + Duration::from_millis(1);
    let mut at = Duration::ZERO;
    for n in 0..10 {
        at += if n == 3 {
            late
        } else {
            Duration::from_millis(60)
        };
        gap.add(at, &quiet);
    }
    assert!(!gap.verdict(WINDOW.max(at)).on_time);
    // The window's end long after its last poll.
    let mut end = Tally::new(&quiet);
    for n in 1..=9 {
        end.add(Duration::from_millis(50) * n, &quiet);
    }
    assert!(!end.verdict(WINDOW).on_time);
    // Ten polls a POLL apart are on time.
    assert!(window(&quiet, |_| quiet.clone()).verdict(WINDOW).on_time);
}
