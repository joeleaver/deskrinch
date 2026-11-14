//! The applications used most, on an empty query: the record of starts
//! the launcher keeps in the test's `state/deskrinch/starts.toml`, what it
//! shows of it, and how it is turned off and forgotten. On
//! [`crate::screen`]'s fixture: its applications are `Plain`, `Term` and
//! `Other` (`org.deskrinch.*.desktop`).

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use deskrinch_desktop_entries::DesktopId;
use deskrinch_start_record::{Store, now};
use deskrinch_test_input::Key;
use deskrinch_test_wait::comes_true;

use crate::screen::{Shown, args, at_centre, bands, results_drawn};
use crate::support::{in_nested_session, launcher, stays};

/// The record's directory under the test's.
fn state(dir: &Path) -> PathBuf {
    dir.join("state/deskrinch")
}

fn record_file(dir: &Path) -> PathBuf {
    state(dir).join("starts.toml")
}

fn id(name: &str) -> DesktopId {
    DesktopId::new(format!("org.deskrinch.{name}.desktop")).unwrap()
}

/// Writes a record in which each of `names` was started more often than
/// the next.
fn write_record(dir: &Path, names: &[&str]) {
    let store = Store::in_directory(state(dir));
    let now = now();
    for (n, name) in names.iter().enumerate() {
        for _ in n..names.len() {
            store.record(&id(name), now).unwrap();
        }
    }
}

/// Whether the record names `name`.
fn records(dir: &Path, name: &str) -> bool {
    fs::read_to_string(record_file(dir)).is_ok_and(|text| text.contains(id(name).as_str()))
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Escape, once the launcher is gone from the screen.
fn hide(shown: &mut Shown) {
    shown.input.press(Key::ESCAPE).unwrap();
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
}

/// The arguments of every application started so far, once there are
/// `count`.
fn started(shown: &Shown, count: usize) -> Vec<Vec<String>> {
    assert!(
        comes_true(|| shown.started().len() >= count),
        "{:?}",
        shown.started()
    );
    let started = shown.started();
    assert_eq!(started.len(), count, "{started:?}");
    started
        .iter()
        .map(|record| args(record).into_iter().map(str::to_owned).collect())
        .collect()
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_started_application_is_first_when_shown_again() {
    let mut shown = Shown::start("");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    assert!(
        comes_true(|| shown
            .launcher
            .said("recorded a start of org.deskrinch.Plain.desktop")),
        "the start was not recorded\nthe launcher logged:\n{}",
        shown.launcher.log()
    );
    assert!(records(shown.dir(), "Plain"));
    assert_eq!(mode(&state(shown.dir())), 0o700);
    assert_eq!(mode(&record_file(shown.dir())), 0o600);
    shown.see("the launcher hidden", |image| at_centre(image).is_none());
    shown.show_again();
    shown.see("the application used, with nothing typed", results_drawn);
    // Selected: Enter starts it again.
    shown.input.press(Key::ENTER).unwrap();
    assert_eq!(started(&shown, 2), [["Plain"], ["Plain"]]);
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn a_hidden_launcher_builds_its_list_and_draws_nothing() {
    let hidden = Shown::hidden("", |dir| write_record(dir, &["Plain", "Other"]));
    assert!(
        stays(|| hidden.launcher.frames() == 0),
        "a hidden launcher drew\nthe launcher logged:\n{}",
        hidden.launcher.log()
    );
    // A forgetting while hidden changes the list, and draws nothing either.
    let forget = launcher(&["forget-starts"])
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(forget.success());
    assert!(
        stays(|| hidden.launcher.frames() == 0),
        "a hidden launcher drew\nthe launcher logged:\n{}",
        hidden.launcher.log()
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn the_best_installed_application_of_the_record_comes_first() {
    let mut shown = Shown::summon("", |dir| write_record(dir, &["Gone", "Other", "Plain"]));
    shown.see("the record's applications", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    assert_eq!(started(&shown, 1), [["Other"]], "Gone is not installed");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn off_deletes_the_record_and_shows_and_records_nothing() {
    let config = "[general]\nremember-starts = false\n";
    let mut shown = Shown::summon(config, |dir| write_record(dir, &["Plain"]));
    assert!(!record_file(shown.dir()).exists(), "the record was kept");
    let empty = shown.see("the launcher drawn", |image| bands(image) > 0);
    assert_eq!(bands(&empty), 1, "only the query field");
    shown.input.type_text("plain").unwrap();
    shown.see("the results of the query", results_drawn);
    shown.input.press(Key::ENTER).unwrap();
    assert_eq!(started(&shown, 1), [["Plain"]]);
    assert!(
        stays(|| !record_file(shown.dir()).exists()),
        "a start was recorded"
    );
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn turning_it_off_while_running_deletes_the_record_and_the_list() {
    let mut shown = Shown::summon("", |dir| write_record(dir, &["Plain"]));
    shown.see("the record's applications", results_drawn);
    hide(&mut shown);
    let config = shown.dir().join("config/deskrinch/config.toml");
    fs::write(config, "[general]\nremember-starts = false\n").unwrap();
    assert!(
        comes_true(|| !record_file(shown.dir()).exists()),
        "the record was kept"
    );
    shown.show_again();
    let empty = shown.see("the launcher drawn", |image| bands(image) > 0);
    assert_eq!(bands(&empty), 1, "only the query field");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn forget_starts_deletes_the_record_and_empties_the_list() {
    let mut shown = Shown::summon("", |dir| write_record(dir, &["Plain"]));
    shown.see("the record's applications", results_drawn);
    hide(&mut shown);
    let forget = launcher(&["forget-starts"])
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(forget.success());
    // Deleted by the time the command returns.
    assert!(!record_file(shown.dir()).exists(), "the record was kept");
    shown.show_again();
    let empty = shown.see("the launcher drawn", |image| bands(image) > 0);
    assert_eq!(bands(&empty), 1, "only the query field");
}

#[test]
#[ignore = "needs a compositor: cargo xtask compositor-tests"]
fn forget_starts_with_none_running_deletes_the_record_and_starts_none() {
    in_nested_session();
    let dir = tempfile::tempdir().unwrap();
    write_record(dir.path(), &["Plain"]);
    let forget = launcher(&["forget-starts"])
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert!(forget.success());
    assert!(!record_file(dir.path()).exists());
    assert!(
        stays(|| crate::support::owner_pid().is_none()),
        "a launcher was started"
    );
}
