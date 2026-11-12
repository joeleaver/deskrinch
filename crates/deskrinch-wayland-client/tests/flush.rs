//! A compositor slow to read is not a lost connection: the client's thread
//! waits until the socket takes the rest. Against a fake compositor on one
//! end of a socket pair, which answers `wl_display.get_registry` (with no
//! globals) and `wl_display.sync`, and can stop reading.

// `cfg(test)` marks it as test code, where clippy allows `unwrap`.
#![cfg(test)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use deskrinch_wayland_client::{Client, EndCause, Ended, Flushed};
use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, QueueHandle};

/// How long anything here waits.
const PATIENCE: Duration = Duration::from_secs(30);
/// Requests sent between flushes: well inside the backend's own buffer, so
/// that only the socket fills.
const BATCH: usize = 64;

/// The fake compositor: answers what a client sends on `stream` while
/// `reading` is set.
fn serve(mut stream: UnixStream, reading: &AtomicBool) {
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let mut pending: Vec<u8> = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        if !reading.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => pending.extend_from_slice(&buffer[..read]),
            Err(_) => continue,
        }
        // Whole messages: object, then size (high half) and opcode.
        while pending.len() >= 8 {
            let word = |at: usize| u32::from_ne_bytes(pending[at..at + 4].try_into().unwrap());
            let (object, size, opcode) = (word(0), (word(4) >> 16) as usize, word(4) & 0xffff);
            if pending.len() < size {
                break;
            }
            // wl_display.sync(new callback): done, then the id is free.
            if object == 1 && opcode == 0 {
                let callback = word(8);
                let mut answer = Vec::new();
                for (sender, opcode, argument) in [(callback, 0, 0), (1, 1, callback)] {
                    answer.extend_from_slice(&sender.to_ne_bytes());
                    answer.extend_from_slice(&((12_u32 << 16) | opcode).to_ne_bytes());
                    answer.extend_from_slice(&u32::to_ne_bytes(argument));
                }
                if stream.write_all(&answer).is_err() {
                    return;
                }
            }
            pending.drain(..size);
        }
    }
}

/// A state that counts answered syncs and records its end.
struct Counter {
    answered: Arc<AtomicUsize>,
    ends: Arc<Mutex<Vec<String>>>,
}

impl Ended for Counter {
    fn end(&mut self, cause: EndCause) {
        self.ends.lock().unwrap().push(format!("{cause:?}"));
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Counter {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

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
    let (ours, theirs) = UnixStream::pair().unwrap();
    let reading = Arc::new(AtomicBool::new(true));
    let peer_reading = Arc::clone(&reading);
    thread::spawn(move || serve(theirs, &peer_reading));

    let client = Client::from_connection(Connection::from_socket(ours).unwrap()).unwrap();
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
    reading.store(false, Ordering::Release);
    let mut sent = 0;
    let deadline = Instant::now() + PATIENCE;
    loop {
        for _ in 0..BATCH {
            client.display().sync(&queue, ());
        }
        sent += BATCH;
        if attachment.flush().unwrap() == Flushed::Pending {
            break;
        }
        assert!(Instant::now() < deadline, "the socket never filled");
    }
    // Waiting on a full socket ends nothing.
    thread::sleep(Duration::from_secs(1));
    assert_eq!(*ends.lock().unwrap(), Vec::<String>::new());

    // The compositor reads again: every request arrives and is answered.
    reading.store(true, Ordering::Release);
    let deadline = Instant::now() + PATIENCE;
    while answered.load(Ordering::Acquire) < sent {
        assert!(
            Instant::now() < deadline,
            "{} of {sent} requests answered",
            answered.load(Ordering::Acquire)
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(*ends.lock().unwrap(), Vec::<String>::new());
    drop(attachment);
}
