//! The client's thread and its states: how they end.

use std::sync::{Arc, Mutex};

use deskrinch_wayland_client::{AttachError, Client};

use crate::support::{OnDone, Probe, attach, bounded, eventually, nested, thread_alive};

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn the_thread_ends_with_the_last_handle() {
    nested();
    let client = Client::connect().unwrap();
    let attached = attach(&client);
    // This client's own thread, by the id its setup took there: no other
    // client's thread can be mistaken for it.
    let ours = attached.record.lock().unwrap().thread.expect("setup ran");
    // The client handle goes first: the attachment keeps the thread. A
    // live thread is always listed.
    drop(client);
    assert!(thread_alive(ours), "the attachment keeps the thread");
    bounded("detaching the last state", move || drop(attached));
    // The last handle's drop joins the thread, but a joined thread may still
    // be listed for a moment: the kernel wakes the joiner when the thread
    // releases its memory, and takes it out of the task list only later, as
    // it finishes exiting. That the drop joins is the client's own business;
    // this sees the thread gone.
    eventually("the client's thread gone", || !thread_alive(ours));
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
