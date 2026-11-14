//! A toplevel the compositor announces after the client asked it to stop
//! is destroyed and never listed, and the state is dropped once `finished`
//! comes. Against a stand-in compositor (`deskrinch_test_wayland::stand_in`),
//! which announces it on cue, as no real one can be made to.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#![cfg(test)]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_test_wait::Deadline;
use deskrinch_test_wayland::stand_in::{Arg, SERVER_ID, StandIn, message};
use deskrinch_window_management::{Client, Event, connect_with};

/// The manager's global name, as the stand-in lists it.
const MANAGER: u32 = 1;
/// `zwlr_foreign_toplevel_manager_v1.stop`.
const STOP: u32 = 0;
/// `zwlr_foreign_toplevel_manager_v1`'s `toplevel` and `finished`.
const TOPLEVEL: u16 = 0;
const FINISHED: u16 = 1;
/// `zwlr_foreign_toplevel_handle_v1.destroy`.
const DESTROY: u32 = 7;

#[test]
fn a_toplevel_announced_after_stop_is_destroyed_and_the_state_dropped() {
    let (connection, stand_in) =
        StandIn::start(&[(MANAGER, "zwlr_foreign_toplevel_manager_v1", 3)]).unwrap();
    let client = Client::from_connection(connection).unwrap();
    let heard = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Arc::clone(&heard);
    let manager = connect_with(&client, move |event| subscriber.lock().unwrap().push(event))
        .expect("the stand-in's window management");
    let [manager_id] = stand_in.bound(MANAGER)[..] else {
        panic!("the manager bound once: {:?}", stand_in.bound(MANAGER));
    };
    let before = heard.lock().unwrap().len();

    drop(manager);
    assert!(
        stand_in.wait_request(manager_id, STOP, Deadline::after(PATIENCE)),
        "the manager told to stop: {:?}",
        stand_in.requests_to(manager_id)
    );
    // Waiting for `finished`.
    let states = client.states();
    assert_eq!((states.attached, states.releasing), (0, 1), "{states:?}");

    // A toplevel the compositor announced before it read the `stop`, then
    // its `finished`.
    let late = SERVER_ID;
    let mut burst = message(manager_id, TOPLEVEL, &[Arg::Uint(late)]);
    burst.extend(message(manager_id, FINISHED, &[]));
    stand_in.send(burst);
    assert!(
        stand_in.wait_request(late, DESTROY, Deadline::after(PATIENCE)),
        "the late toplevel destroyed: {:?}",
        stand_in.requests_to(late)
    );
    let deadline = Instant::now() + PATIENCE;
    loop {
        let states = client.states();
        if (states.attached, states.releasing) == (0, 0) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the state never dropped: {states:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let after: Vec<Event> = heard.lock().unwrap()[before..].to_vec();
    assert!(
        !after.iter().any(|event| matches!(event, Event::Added(_))),
        "the late toplevel was listed: {after:?}"
    );
}
