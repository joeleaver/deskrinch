//! A compositor slow to read is not a lost connection: the client's thread
//! waits until the socket takes the rest. Against a stand-in compositor
//! (`deskrinch_test_wayland::stand_in`) with no globals, which can stop reading.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#![cfg(test)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use deskrinch_wayland_client::{Client, EndCause, Flushed, Globals, Lifecycle};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::{Connection, Dispatch, QueueHandle};

use deskrinch_test_wait::{Deadline, QUIET, wait_for};
use deskrinch_test_wayland::stand_in::StandIn;

/// Requests sent between flushes: well inside the backend's own buffer, so
/// that only the socket fills.
const BATCH: usize = 64;

/// A state that counts answered syncs and records its end.
struct Counter {
    answered: Arc<AtomicUsize>,
    ends: Arc<Mutex<Vec<String>>>,
}

impl Lifecycle for Counter {
    fn end(&mut self, cause: EndCause) {
        self.ends.lock().unwrap().push(format!("{cause:?}"));
    }

    // It binds nothing: its syncs' callbacks are gone once answered.
    fn release(&mut self) {}

    fn released(&self) -> bool {
        true
    }
}

impl Globals for Counter {}

impl Dispatch<WlCallback, ()> for Counter {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        _: wl_callback::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.answered.fetch_add(1, Ordering::AcqRel);
    }
}

#[test]
fn a_full_socket_is_waited_out_not_taken_for_a_lost_connection() {
    let (connection, stand_in) = StandIn::start(&[]).unwrap();
    let client = Client::from_connection(connection).unwrap();
    let answered = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(Mutex::new(Vec::new()));
    let state = Counter {
        answered: Arc::clone(&answered),
        ends: Arc::clone(&ends),
    };
    let (attachment, queue) = client
        .attach(move |setup| Ok::<_, ()>((state, setup.handle())))
        .unwrap();

    // The compositor stops reading; requests go out until the socket is full.
    stand_in.set_reading(false);
    let mut sent = 0;
    let deadline = Deadline::patience();
    loop {
        for _ in 0..BATCH {
            client.display().sync(&queue, ());
        }
        sent += BATCH;
        if attachment.flush().unwrap() == Flushed::Pending {
            break;
        }
        assert!(!deadline.passed(), "the socket never filled");
    }
    // Waiting on a full socket ends nothing.
    thread::sleep(QUIET);
    assert_eq!(*ends.lock().unwrap(), Vec::<String>::new());

    // The compositor reads again: every request arrives and is answered.
    stand_in.set_reading(true);
    wait_for(&format!("all {sent} requests to be answered"), || {
        let count = answered.load(Ordering::Acquire);
        if count < sent { Err(count) } else { Ok(()) }
    });
    assert_eq!(*ends.lock().unwrap(), Vec::<String>::new());
    drop(attachment);
}
