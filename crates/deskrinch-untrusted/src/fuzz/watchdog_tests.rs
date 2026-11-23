//! The watchdog's own tests, each in a child process (this test binary run
//! again for one inner test), since what is tested is how a process ends
//! and how many threads it keeps.

use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use deskrinch_test_wait::{PATIENCE, eventually};

use super::{Input, within};

/// A structured input, for the text form.
#[derive(Debug)]
struct Tree {
    #[expect(dead_code, reason = "read only through Debug")]
    depth: u32,
}

/// The directory an inner test reports into, set by its outer test.
const DIR: &str = "DESKRINCH_FUZZ_WATCHDOG_DIR";

/// This test binary run again for the ignored inner test `name`, reporting
/// into `dir`: its output, its process id, and how long it took. libtest
/// captures what the test prints, as in an ordinary run: the watchdog's
/// sentence must reach standard error all the same.
fn child(name: &str, dir: &Path) -> (Output, u32, Duration) {
    let started = Instant::now();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--test-threads", "1"])
        .env(DIR, dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    (output, pid, started.elapsed())
}

fn dir() -> std::path::PathBuf {
    std::env::var_os(DIR).expect("run by its outer test").into()
}

#[test]
#[ignore = "run by a_case_past_its_limit_aborts_with_its_bytes_saved, in a child"]
fn inner_case_that_hangs_on_bytes() {
    within(
        "inner::bytes",
        &Input::Bytes(&[1, 2, 3, 255]),
        Duration::from_secs(1),
        &dir(),
        || {
            std::thread::sleep(Duration::from_secs(30));
        },
    );
}

#[test]
#[ignore = "run by a_case_past_its_limit_aborts_with_its_text_saved, in a child"]
fn inner_case_that_hangs_on_text() {
    within(
        "inner::text",
        &Input::Text(&Tree { depth: 3 }),
        Duration::from_secs(1),
        &dir(),
        || {
            std::thread::sleep(Duration::from_secs(30));
        },
    );
}

/// The limit of the cases in time. A watchdog left behind (one not stood
/// down when its case returns) acts only at its limit, so it must still be
/// waiting, and still listed, when the wait for the task list
/// ([`PATIENCE`]) runs out: the patience and a minute more.
const IN_TIME_LIMIT: Duration = PATIENCE.saturating_add(Duration::from_secs(60));

#[test]
#[ignore = "run by cases_in_time_leave_nothing_and_no_watchdog_behind, in a child"]
fn inner_cases_in_time() {
    let threads = || std::fs::read_dir("/proc/thread-self/..").unwrap().count();
    let before = threads();
    for i in 0..200_u32 {
        if i % 10 == 0 {
            // A case that fails (a panic, which proptest shrinks) stands the
            // watchdog down too, as it unwinds.
            let panicked = std::panic::catch_unwind(|| {
                within(
                    "inner::panics",
                    &Input::Bytes(&[1]),
                    IN_TIME_LIMIT,
                    &dir(),
                    || {
                        panic!("an ordinary failure");
                    },
                );
            });
            assert!(panicked.is_err());
        } else {
            within(
                "inner::in_time",
                &Input::Bytes(&[2]),
                IN_TIME_LIMIT,
                &dir(),
                || {},
            );
        }
    }
    // A thread whose closure has returned (the scope's end waits for that)
    // stays listed until the system has reaped it. Read at once, the count
    // was higher in 7 runs of 500, in 498 of 500 pinned to one CPU and in 39
    // of 300 with every CPU busy (2026-11-23); it came back within 3.3 ms
    // every time. So the list is waited for, not read once.
    eventually("the task list back to its count before the cases", || {
        threads() == before
    });
}

/// What the abort's outer tests check: the process ended by `SIGABRT`
/// within a few seconds of its 1 s limit (its case would sleep 30 s), said
/// so in one sentence, and saved the input as `extension`.
fn assert_aborted(
    output: &Output,
    pid: u32,
    took: Duration,
    dir: &Path,
    (property, file_name): (&str, &str),
    extension: &str,
    saved: &[u8],
) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.signal(), Some(libc::SIGABRT), "{stderr}");
    assert!(took < Duration::from_secs(10), "took {took:?}");
    let file = dir.join(format!("deskrinch-fuzz-hang-{file_name}-{pid}.{extension}"));
    assert!(
        stderr.contains(&format!(
            "fuzz watchdog: a case of {} ran past its 1 s limit and the test binary is aborted; its \
             input is in {}",
            property,
            file.display()
        )),
        "{stderr}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), saved);
}

#[test]
fn a_case_past_its_limit_aborts_with_its_bytes_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (output, pid, took) = child(
        "fuzz::watchdog::tests::inner_case_that_hangs_on_bytes",
        dir.path(),
    );
    assert_aborted(
        &output,
        pid,
        took,
        dir.path(),
        ("inner::bytes", "inner--bytes"),
        "bin",
        &[1, 2, 3, 255],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("give them to the code under test again to replay it"),
        "{stderr}"
    );
}

#[test]
fn a_case_past_its_limit_aborts_with_its_text_saved() {
    let dir = tempfile::tempdir().unwrap();
    let (output, pid, took) = child(
        "fuzz::watchdog::tests::inner_case_that_hangs_on_text",
        dir.path(),
    );
    assert_aborted(
        &output,
        pid,
        took,
        dir.path(),
        ("inner::text", "inner--text"),
        "txt",
        format!("{:#?}", Tree { depth: 3 }).as_bytes(),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("its input's Debug text, to rebuild the case by hand"),
        "{stderr}"
    );
}

#[test]
fn cases_in_time_leave_nothing_and_no_watchdog_behind() {
    let dir = tempfile::tempdir().unwrap();
    let (output, _, _) = child("fuzz::watchdog::tests::inner_cases_in_time", dir.path());
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "a case in time saved its input"
    );
}
