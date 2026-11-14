//! A process killed with `SIGKILL` takes the child it started with `spawn`
//! with it, and leaves its `owned_dir` to the next sweep.
//!
//! The killed process is this test binary run again, as a child: its
//! ignored test `killed_process`, started with an environment variable that
//! lets it run.

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use deskrinch_test_lifetime::{owned_dir, running, spawn};
use deskrinch_test_wait::eventually;

/// Set in the child's environment, so that the ignored test runs only
/// there.
const CHILD: &str = "DESKRINCH_TEST_LIFETIME_CHILD";
/// What the child prints before what it reports.
const MARK: &str = "killed-process:";

/// The process to kill; not a test of its own.
#[test]
#[ignore = "run in a child process by a_killed_process_takes_its_child_and_leaves_its_directory"]
#[expect(
    clippy::zombie_processes,
    reason = "this process is killed while its child runs: the kernel kills the child, and its new parent reaps it"
)]
fn killed_process() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // Started from a thread that then ends: the child outlives the thread,
    // as a bus shared by tests outlives the test that started it.
    let sleeper = thread::spawn(|| {
        let mut command = Command::new("sleep");
        command.arg("600");
        spawn(command).unwrap()
    })
    .join()
    .unwrap();
    let dir = owned_dir().unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(running(sleeper.id()), "the child ended with its thread");
    println!("{MARK} {} {}", sleeper.id(), dir.path().display());
    // Until killed (or the test closes standard input).
    let _read = std::io::stdin().read_to_end(&mut Vec::new());
}

#[test]
fn a_killed_process_takes_its_child_and_leaves_its_directory() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "killed_process", "--ignored", "--nocapture"])
        .env(CHILD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // Itself bound to this process, should this test be killed.
    let mut process = spawn(command).unwrap();
    let stdout = process.stdout.take().unwrap();
    let report = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix(MARK).map(str::to_owned))
        .expect("the process reported its child and directory");
    let (pid, dir) = report.trim().split_once(' ').unwrap();
    let (pid, dir): (u32, PathBuf) = (pid.parse().unwrap(), dir.into());
    assert!(
        command_line(pid).contains("sleep"),
        "{pid} is not the child"
    );
    assert!(dir.is_dir());

    process.kill().unwrap();
    process.wait().unwrap();

    // Gone, not gone quickly: the kernel kills it as its parent dies, but it
    // still has to be scheduled to exit, which a loaded machine can delay.
    eventually("the child to go with its process", || !running(pid));
    // Its directory stays until the next process makes one.
    assert!(dir.is_dir());
    let _mine = owned_dir().unwrap();
    assert!(!dir.exists(), "the sweep left {}", dir.display());
}

fn command_line(pid: u32) -> String {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
        .unwrap_or_default()
}
