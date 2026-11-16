//! The capture's two promises: each thread reads only its own lines, and a
//! callsite first hit on a thread that does not capture is still recorded
//! for one that does.

use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn two_threads_capturing_at_once_each_read_only_their_own_lines() {
    let barrier = Arc::new(Barrier::new(2));
    let each = |name: &'static str| {
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let capture = deskrinch_test_log::capture();
            barrier.wait();
            for i in 0..100 {
                tracing::info!("{name} {i}");
            }
            barrier.wait();
            capture.lines()
        })
    };
    let (a, b) = (each("a"), each("b"));
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    let want = |name: &str| (0..100).map(|i| format!("{name} {i}")).collect::<Vec<_>>();
    assert_eq!(a, want("a"));
    assert_eq!(b, want("b"));
}

#[test]
fn an_event_on_a_thread_not_capturing_is_in_no_one_s_lines() {
    let capture = deskrinch_test_log::capture();
    thread::spawn(|| tracing::info!("elsewhere"))
        .join()
        .unwrap();
    tracing::info!("here");
    assert_eq!(capture.lines(), ["here"]);
}

#[test]
fn the_lines_end_with_the_capture() {
    {
        let capture = deskrinch_test_log::capture();
        tracing::info!("first");
        assert_eq!(capture.lines(), ["first"]);
    }
    tracing::info!("between");
    let capture = deskrinch_test_log::capture();
    tracing::info!("second");
    assert_eq!(capture.lines(), ["second"]);
}

/// 64 callsites, each its own `tracing::info!`, called by index.
macro_rules! callsites {
    ($($n:literal)*) => {
        [$(|| tracing::info!("callsite {}", $n),)*]
    };
}

/// The bug's shape: with a subscriber scoped to the capturing thread, a
/// callsite first hit on a thread without one was cached as uninteresting
/// for the whole process, and the capturing thread's event at it was lost.
/// Each callsite here is first hit on a thread that does not capture.
#[test]
fn a_callsite_first_hit_on_a_thread_not_capturing_is_recorded_for_one_that_is() {
    let callsites: [fn(); 64] = callsites!(
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
        32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59
        60 61 62 63
    );
    let capture = deskrinch_test_log::capture();
    for (n, callsite) in callsites.iter().enumerate() {
        thread::spawn(*callsite).join().unwrap();
        callsite();
        assert_eq!(
            capture.lines().last().map(String::as_str),
            Some(format!("callsite {n}").as_str()),
            "callsite {n}"
        );
    }
    assert_eq!(capture.lines().len(), callsites.len());
}

#[test]
#[should_panic(expected = "already capturing")]
fn a_second_capture_on_one_thread_panics() {
    let _first = deskrinch_test_log::capture();
    let _second = deskrinch_test_log::capture();
}
