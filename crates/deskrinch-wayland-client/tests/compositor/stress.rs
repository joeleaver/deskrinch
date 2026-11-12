//! Many states attached to and detached from one client, from several
//! threads at once: the race that made a thread wait for events already in
//! its queue had its best chance here, and every wait is bounded.

use std::thread;

use deskrinch_wayland_client::Client;

use crate::support::{OnDone, attach, bounded, nested};

/// Rounds each thread runs.
const ROUNDS: usize = 200;
/// Threads attaching at once: two crates' worth, and more.
const THREADS: usize = 4;

#[test]
#[ignore = "needs a nested compositor: cargo xtask compositor-tests"]
fn states_attach_and_detach_on_one_client_without_waiting_forever() {
    nested();
    let client = Client::connect().unwrap();
    // One state attached throughout, as a crate that stays.
    let staying = attach(&client);
    let threads: Vec<_> = (0..THREADS)
        .map(|_| {
            let client = client.clone();
            thread::spawn(move || {
                for _ in 0..ROUNDS {
                    let attached = attach(&client);
                    assert!(attached.record.lock().unwrap().outputs > 0);
                    // An exchange on the state's own queue, answered on the
                    // client's thread, while others attach and detach.
                    client.display().sync(&attached.queue, OnDone::Nothing);
                    attached.attachment.flush().unwrap();
                    bounded("detaching a probe", move || drop(attached));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("a stress thread failed");
    }
    assert_eq!(staying.record.lock().unwrap().ends, Vec::<String>::new());
    bounded("detaching the staying probe", move || drop(staying));
}
