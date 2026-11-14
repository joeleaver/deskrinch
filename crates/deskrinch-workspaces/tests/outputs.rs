//! A workspace group's outputs on one connection that lives through an
//! output going, as a panel's does: against a stand-in compositor
//! (`deskrinch_test_wayland::stand_in`), which sends the events in an order
//! no real one can be made to on cue.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#![cfg(test)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_wayland::stand_in::{Arg, SERVER_ID, StandIn, message};
use deskrinch_workspaces::{Client, Event, Group, connect_with};

/// How long anything here waits.
const PATIENCE: Duration = Duration::from_secs(30);
/// The globals, as the stand-in lists them.
const OUTPUT: u32 = 1;
const MANAGER: u32 = 2;
/// `wl_registry.global_remove`.
const GLOBAL_REMOVE: u16 = 1;
/// `wl_output`'s `done` and `name`.
const OUTPUT_DONE: u16 = 2;
const OUTPUT_NAME: u16 = 4;
/// `ext_workspace_manager_v1`'s `workspace_group` and `done`.
const WORKSPACE_GROUP: u16 = 0;
const MANAGER_DONE: u16 = 2;
/// `ext_workspace_group_handle_v1.output_enter`.
const OUTPUT_ENTER: u16 = 1;

/// The group as the subscriber last heard of it.
fn last_heard(heard: &Mutex<Vec<Event>>) -> Option<Group> {
    heard
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::GroupChanged(group) => Some(group.clone()),
            _ => None,
        })
}

/// Waits until the group is heard of on `outputs`.
fn wait_outputs(heard: &Mutex<Vec<Event>>, outputs: &[&str]) {
    let wanted: BTreeSet<String> = outputs.iter().map(|&name| name.to_owned()).collect();
    let deadline = Instant::now() + PATIENCE;
    loop {
        let last = last_heard(heard);
        if last.as_ref().is_some_and(|group| group.outputs == wanted) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "waited for the group on {wanted:?}; last heard {last:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_group_is_off_an_output_whose_global_goes_with_no_leave() {
    let (connection, stand_in) = StandIn::start(&[
        (OUTPUT, "wl_output", 4),
        (MANAGER, "ext_workspace_manager_v1", 1),
    ])
    .unwrap();
    let client = Client::from_connection(connection).unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Arc::clone(&heard);
    let _manager = connect_with(&client, move |event| subscriber.lock().unwrap().push(event))
        .expect("the stand-in's workspaces");
    let [output] = stand_in.bound(OUTPUT)[..] else {
        panic!("the output bound once: {:?}", stand_in.bound(OUTPUT));
    };
    let [manager] = stand_in.bound(MANAGER)[..] else {
        panic!("the manager bound once: {:?}", stand_in.bound(MANAGER));
    };
    let registry = stand_in.registry().expect("the client's registry");

    // A group on the output.
    let group = SERVER_ID;
    let mut burst = message(output, OUTPUT_NAME, &[Arg::Str("OUT")]);
    burst.extend(message(output, OUTPUT_DONE, &[]));
    burst.extend(message(manager, WORKSPACE_GROUP, &[Arg::Uint(group)]));
    burst.extend(message(group, OUTPUT_ENTER, &[Arg::Uint(output)]));
    burst.extend(message(manager, MANAGER_DONE, &[]));
    stand_in.send(burst);
    wait_outputs(&heard, &["OUT"]);

    // The output goes, and nothing names it again: the group is reported on
    // no output at once, not at some later `done`.
    stand_in.send(message(registry, GLOBAL_REMOVE, &[Arg::Uint(OUTPUT)]));
    wait_outputs(&heard, &[]);
}
