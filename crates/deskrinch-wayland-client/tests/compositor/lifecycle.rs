//! The client's thread and its states: how they end.

use std::sync::{Arc, Mutex};

use deskrinch_wayland_client::{AttachError, Client};

use crate::support::{
    OnDone, Probe, attach, bounded, client_threads, eventually, nested, thread_alive,
};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_thread_ends_with_the_last_handle() {
    nested();
    // This client's own thread, by its id: another client's thread (an
    // earlier test's, say) being born or ending meanwhile changes nothing
    // here.
    let before = client_threads();
    let client = Client::connect().unwrap();
    // The thread names itself once it runs.
    let ours = Mutex::new(None);
    eventually("the client's thread named", || {
        let new: Vec<u32> = client_threads().difference(&before).copied().collect();
        if let [id] = new.as_slice() {
            *ours.lock().unwrap() = Some(*id);
        }
        new.len() == 1
    });
    let ours = ours.into_inner().unwrap().unwrap();
    let attached = attach(&client);
    // The client handle goes first: the attachment keeps the thread.
    drop(client);
    assert!(thread_alive(ours), "the attachment keeps the thread");
    bounded("detaching the last state", move || drop(attached));
    // The last handle's drop joins the thread: it is gone at once.
    assert!(!thread_alive(ours), "the thread outlived its last handle");
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_panicking_handler_ends_every_state_once() {
    nested();
    let client = Client::connect().unwrap();
    let panicking = attach(&client);
    let bystander = attach(&client);
    client.display().sync(&panicking.queue, OnDone::Panic);
    panicking.attachment.flush().unwrap();
    for record in [&panicking.record, &bystander.record] {
        eventually("every state ended", || {
            !record.lock().unwrap().ends.is_empty()
        });
        let ends = record.lock().unwrap().ends.clone();
        assert_eq!(ends, ["HandlerPanicked"], "ended once, for the panic");
    }
    // Nothing more attaches to an ended connection, and saying so does not
    // hang.
    let again = client.clone();
    let refused = bounded("attaching after the end", move || {
        again
            .attach(|_| Err::<(Probe, ()), ()>(()))
            .map(|_| ())
            .unwrap_err()
    });
    assert!(matches!(refused, AttachError::Ended), "{refused:?}");
    bounded("detaching ended states", move || {
        drop(panicking);
        drop(bystander);
    });
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_handle_dropped_inside_its_own_handler_does_not_deadlock() {
    nested();
    let client = Client::connect().unwrap();
    let attached = attach(&client);
    let held = Arc::new(Mutex::new(Some(attached.attachment)));
    client
        .display()
        .sync(&attached.queue, OnDone::Detach(Arc::clone(&held)));
    held.lock()
        .unwrap()
        .as_ref()
        .expect("still held: the answer cannot come before the flush")
        .flush()
        .unwrap();
    eventually("the handle dropped by its handler", || {
        held.lock().unwrap().is_none()
    });
    // The client goes on: another state attaches and is dispatched.
    let other = attach(&client);
    assert!(other.record.lock().unwrap().outputs > 0);
    assert_eq!(attached.record.lock().unwrap().ends, Vec::<String>::new());
    bounded("detaching the other state", move || drop(other));
}
