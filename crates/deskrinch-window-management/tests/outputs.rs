//! A window's outputs on one connection that lives through an output going
//! and coming back, as a panel's does: against a stand-in compositor
//! (`deskrinch_test_wayland::stand_in`), which sends the events in an order
//! no real one can be made to on cue.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#![cfg(test)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_wayland::stand_in::{Arg, SERVER_ID, StandIn, message};
use deskrinch_window_management::{Client, Event, Window, connect_with};

/// How long anything here waits.
const PATIENCE: Duration = Duration::from_secs(30);
/// The globals, as the stand-in lists them: the output, the manager, and
/// the output again under a new name once it comes back.
const OUTPUT: u32 = 1;
const MANAGER: u32 = 2;
const OUTPUT_AGAIN: u32 = 3;
/// `wl_registry`'s `global` and `global_remove`.
const GLOBAL: u16 = 0;
const GLOBAL_REMOVE: u16 = 1;
/// `wl_output`'s `done` and `name`.
const OUTPUT_DONE: u16 = 2;
const OUTPUT_NAME: u16 = 4;
/// `zwlr_foreign_toplevel_manager_v1.toplevel`.
const TOPLEVEL: u16 = 0;
/// `zwlr_foreign_toplevel_handle_v1`'s `title`, `app_id`, `output_enter`,
/// `output_leave` and `done`.
const TITLE: u16 = 0;
const APP_ID: u16 = 1;
const OUTPUT_ENTER: u16 = 2;
const OUTPUT_LEAVE: u16 = 3;
const DONE: u16 = 5;

/// The window as the subscriber last heard of it.
fn last_heard(heard: &Mutex<Vec<Event>>) -> Option<Window> {
    heard
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Added(window) | Event::Changed(window) => Some(window.clone()),
            _ => None,
        })
}

/// Waits until the window is heard of on `outputs`.
fn wait_outputs(heard: &Mutex<Vec<Event>>, outputs: &[&str]) {
    let wanted: BTreeSet<String> = outputs.iter().map(|&name| name.to_owned()).collect();
    let deadline = Instant::now() + PATIENCE;
    loop {
        let last = last_heard(heard);
        if last.as_ref().is_some_and(|window| window.outputs == wanted) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "waited for the window on {wanted:?}; last heard {last:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// The one id the client gave global `name`, once it has bound it.
fn bound(stand_in: &StandIn, name: u32) -> u32 {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let [id] = stand_in.bound(name)[..] {
            return id;
        }
        assert!(Instant::now() < deadline, "global {name} never bound once");
        thread::sleep(Duration::from_millis(10));
    }
}

/// An output's name, and its `done`.
fn named(output: u32, name: &str) -> Vec<u8> {
    let mut bytes = message(output, OUTPUT_NAME, &[Arg::Str(name)]);
    bytes.extend(message(output, OUTPUT_DONE, &[]));
    bytes
}

#[test]
fn a_window_follows_an_output_that_goes_and_comes_back() {
    let (connection, stand_in) = StandIn::start(&[
        (OUTPUT, "wl_output", 4),
        (MANAGER, "zwlr_foreign_toplevel_manager_v1", 3),
    ])
    .unwrap();
    let client = Client::from_connection(connection).unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Arc::clone(&heard);
    let _manager = connect_with(&client, move |event| subscriber.lock().unwrap().push(event))
        .expect("the stand-in's window management");
    let output = bound(&stand_in, OUTPUT);
    let manager = bound(&stand_in, MANAGER);
    let registry = stand_in.registry().expect("the client's registry");

    // A window on the output.
    let window = SERVER_ID;
    let mut burst = named(output, "OUT");
    burst.extend(message(manager, TOPLEVEL, &[Arg::Uint(window)]));
    burst.extend(message(window, TITLE, &[Arg::Str("Travelling")]));
    burst.extend(message(window, APP_ID, &[Arg::Str("org.deskrinch.test")]));
    burst.extend(message(window, OUTPUT_ENTER, &[Arg::Uint(output)]));
    burst.extend(message(window, DONE, &[]));
    stand_in.send(burst);
    wait_outputs(&heard, &["OUT"]);

    // The output goes, and comes back as a new global (named apart here, so
    // that what is reported shows which one it is); the window leaves the
    // old one only after it is gone.
    let mut burst = message(registry, GLOBAL_REMOVE, &[Arg::Uint(OUTPUT)]);
    burst.extend(message(
        registry,
        GLOBAL,
        &[Arg::Uint(OUTPUT_AGAIN), Arg::Str("wl_output"), Arg::Uint(4)],
    ));
    stand_in.send(burst);
    let again = bound(&stand_in, OUTPUT_AGAIN);
    let mut burst = named(again, "OUT-AGAIN");
    burst.extend(message(window, OUTPUT_ENTER, &[Arg::Uint(again)]));
    burst.extend(message(window, OUTPUT_LEAVE, &[Arg::Uint(output)]));
    burst.extend(message(window, DONE, &[]));
    stand_in.send(burst);
    wait_outputs(&heard, &["OUT-AGAIN"]);

    // Leaving the new one leaves it on none: nothing of the old global is
    // left behind.
    let mut burst = message(window, OUTPUT_LEAVE, &[Arg::Uint(again)]);
    burst.extend(message(window, DONE, &[]));
    stand_in.send(burst);
    wait_outputs(&heard, &[]);
}
