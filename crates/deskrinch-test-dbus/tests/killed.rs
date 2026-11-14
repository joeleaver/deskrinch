//! A test process killed with `SIGKILL` takes its bus's daemon with it,
//! even a bus started on a thread that has ended, and the next bus started
//! removes the bus's directory.
//!
//! The killed process is this test binary run again, as a child: its
//! ignored test `process_with_a_bus`, started with an environment variable
//! that lets it run.

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use deskrinch_test_dbus::PrivateBus;
use deskrinch_test_lifetime::running;
use deskrinch_test_wait::eventually;

/// Set in the child's environment, so that the ignored test runs only
/// there.
const CHILD: &str = "DESKRINCH_TEST_DBUS_KILLED_CHILD";
/// What the child prints before the bus's session root.
const MARK: &str = "process-with-a-bus:";

/// The process to kill; not a test of its own.
#[test]
#[ignore = "run in a child process by a_killed_process_takes_its_bus_with_it"]
fn process_with_a_bus() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // Started on a thread that then ends, as a bus shared by tests may be.
    let bus = thread::spawn(|| PrivateBus::start().unwrap())
        .join()
        .unwrap();
    // Still serving once that thread has gone.
    thread::sleep(Duration::from_millis(100));
    let _serving =
        zbus::block_on(zbus::connection::Builder::address(bus.address()).build()).unwrap();
    println!("{MARK} {}", bus.session_root().display());
    // Until killed (or the test closes standard input).
    let _read = std::io::stdin().read_to_end(&mut Vec::new());
}

#[test]
fn a_killed_process_takes_its_bus_with_it() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "process_with_a_bus", "--ignored", "--nocapture"])
        .env(CHILD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    // Itself bound to this process, should this test be killed.
    let mut process = deskrinch_test_lifetime::spawn(command).unwrap();
    let stdout = process.stdout.take().unwrap();
    let root: PathBuf = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix(MARK).map(|r| r.trim().into()))
        .expect("the process reported its bus");
    let daemon = daemon_of(&root).expect("the bus's daemon runs");

    process.kill().unwrap();
    process.wait().unwrap();

    // Gone, not gone quickly: the kernel kills it as its parent dies, but it
    // still has to be scheduled to exit, which a loaded machine can delay.
    eventually("the daemon gone with its process", || !running(daemon));
    // Its directory stays until the next bus is started.
    assert!(root.is_dir());
    let _next = PrivateBus::start().unwrap();
    assert!(!root.exists(), "{} was left", root.display());
}

/// The `dbus-daemon` running the configuration under `root`.
fn daemon_of(root: &std::path::Path) -> Option<u32> {
    let config = format!("--config-file={}/bus.conf", root.display());
    fs::read_dir("/proc").ok()?.flatten().find_map(|entry| {
        let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
        let line = fs::read(entry.path().join("cmdline")).ok()?;
        let line = String::from_utf8_lossy(&line);
        line.split('\0').any(|arg| arg == config).then_some(pid)
    })
}
