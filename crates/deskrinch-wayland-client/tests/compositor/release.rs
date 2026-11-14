//! Detached states: released at once, kept releasing while something can
//! reach them, never keeping the connection, and as loud as any when a
//! handler of theirs panics.

use deskrinch_wayland_client::{Client, StateCounts};

use deskrinch_test_wait::eventually;

use crate::support::{Attached, OnDone, attach, bounded, nested, thread_alive};

/// `client`'s attached and releasing states.
fn counts(client: &Client) -> (usize, usize) {
    let StateCounts {
        attached,
        releasing,
        ..
    } = client.states();
    (attached, releasing)
}

/// Attaches a probe whose release leaves it releasing, as one holding an
/// object without a destructor would be.
fn lingering(client: &Client) -> Attached {
    let attached = attach(client);
    attached.record.lock().unwrap().linger = true;
    attached
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_released_state_is_dropped_and_a_lingering_one_kept() {
    nested();
    let client = Client::connect().unwrap();
    let kept = attach(&client);
    let released = attach(&client);
    let lingers = lingering(&client);
    assert_eq!(counts(&client), (3, 0));
    let record = std::sync::Arc::clone(&released.record);
    bounded("detaching a state", move || drop(released.attachment));
    // Its outputs released, nothing else bound: dropped as it was detached.
    assert_eq!(counts(&client), (2, 0));
    assert_eq!(record.lock().unwrap().releases, 1);
    bounded("detaching a lingering state", move || {
        drop(lingers.attachment);
    });
    assert_eq!(counts(&client), (1, 1));
    bounded("detaching the last", move || drop(kept));
    assert_eq!(counts(&client), (0, 1));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_connection_of_its_own_closes_with_its_last_handle() {
    nested();
    let client = Client::connect().unwrap();
    let lingers = lingering(&client);
    let ours = lingers.record.lock().unwrap().thread.expect("setup ran");
    let still = client.clone();
    bounded("detaching the state", move || drop(lingers));
    // Releasing, and never to be released.
    assert_eq!(counts(&still), (0, 1));
    drop(still);
    // The last handle: the thread ends without waiting for the releasing
    // state (as `lifecycle` says, a joined thread may be listed a moment).
    drop(client);
    eventually("the client's thread gone", || !thread_alive(ours));
}

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn a_panic_in_a_releasing_state_ends_the_others() {
    nested();
    let client = Client::connect().unwrap();
    let releasing = lingering(&client);
    let bystander = attach(&client);
    let Attached {
        attachment,
        queue,
        record,
    } = releasing;
    bounded("detaching the state", move || drop(attachment));
    assert_eq!(counts(&client), (1, 1));
    // Its queue is still dispatched: a callback answered there now panics.
    client.display().sync(&queue, OnDone::Panic);
    bystander.attachment.flush().unwrap();
    eventually("the bystander ended", || {
        !bystander.record.lock().unwrap().ends.is_empty()
    });
    assert_eq!(bystander.record.lock().unwrap().ends, ["HandlerPanicked"]);
    // A releasing state has no one to tell.
    assert_eq!(record.lock().unwrap().ends, Vec::<String>::new());
    bounded("detaching the bystander", move || drop(bystander));
}
