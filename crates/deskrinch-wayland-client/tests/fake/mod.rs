//! A stand-in compositor on one end of a socket pair, for what a real one
//! cannot be made to do on cue: stop reading, or send given events in one
//! write.
//!
//! It answers `wl_display.sync`, lists its globals on
//! `wl_display.get_registry`, notes the id each `wl_registry.bind` gives a
//! global, notes every request it reads by its object and opcode, and
//! answers nothing else. A test writes events of its own
//! through [`Fake::send`].

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use wayland_client::Connection;

/// An argument of a message.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Arg<'a> {
    /// A `uint`, `int` or object id.
    Uint(u32),
    /// A `string`.
    Str(&'a str),
}

/// The bytes of the message `opcode` from (or to) `object`.
pub(crate) fn message(object: u32, opcode: u16, args: &[Arg<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for arg in args {
        match *arg {
            Arg::Uint(value) => body.extend_from_slice(&value.to_ne_bytes()),
            Arg::Str(text) => {
                let length = u32::try_from(text.len() + 1).unwrap();
                body.extend_from_slice(&length.to_ne_bytes());
                body.extend_from_slice(text.as_bytes());
                body.push(0);
                while body.len() % 4 != 0 {
                    body.push(0);
                }
            }
        }
    }
    let size = u32::try_from(8 + body.len()).unwrap();
    let mut bytes = Vec::with_capacity(8 + body.len());
    bytes.extend_from_slice(&object.to_ne_bytes());
    bytes.extend_from_slice(&((size << 16) | u32::from(opcode)).to_ne_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

/// What the test and the stand-in share.
#[derive(Debug, Default)]
struct Seen {
    /// The client's `wl_registry`, once asked for.
    registry: Option<u32>,
    /// Each global bound, by its name: the ids the client gave it, one per
    /// bind.
    bound: HashMap<u32, Vec<u32>>,
    /// Every request read: its object, and its opcode.
    requests: Vec<(u32, u32)>,
}

/// The stand-in compositor, as a test drives it.
#[derive(Debug)]
pub(crate) struct Fake {
    reading: Arc<AtomicBool>,
    seen: Arc<Mutex<Seen>>,
    outbox: Sender<Vec<u8>>,
}

impl Fake {
    /// Starts a stand-in offering `globals` (name, interface, version), and
    /// a connection to it.
    pub(crate) fn start(globals: &[(u32, &'static str, u32)]) -> (Connection, Self) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let (outbox, sent) = mpsc::channel();
        let fake = Self {
            reading: Arc::new(AtomicBool::new(true)),
            seen: Arc::default(),
            outbox,
        };
        let reading = Arc::clone(&fake.reading);
        let seen = Arc::clone(&fake.seen);
        let globals = globals.to_vec();
        thread::spawn(move || serve(theirs, &globals, &reading, &seen, &sent));
        (Connection::from_socket(ours).unwrap(), fake)
    }

    /// Whether it reads what the client sends.
    pub(crate) fn set_reading(&self, reading: bool) {
        self.reading.store(reading, Ordering::Release);
    }

    /// The client's `wl_registry`'s id, once it has asked for one.
    pub(crate) fn registry(&self) -> Option<u32> {
        self.seen.lock().unwrap().registry
    }

    /// The ids the client gave the global named `name`, one per bind.
    pub(crate) fn bound(&self, name: u32) -> Vec<u32> {
        self.seen
            .lock()
            .unwrap()
            .bound
            .get(&name)
            .cloned()
            .unwrap_or_default()
    }

    /// The opcodes of the requests read so far on `object`, in order.
    pub(crate) fn requests_to(&self, object: u32) -> Vec<u32> {
        self.seen
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|&&(to, _)| to == object)
            .map(|&(_, opcode)| opcode)
            .collect()
    }

    /// Writes `bytes` to the client, in one write.
    pub(crate) fn send(&self, bytes: Vec<u8>) {
        self.outbox.send(bytes).unwrap();
    }
}

/// The stand-in's thread: writes what the test sends, and reads and
/// answers the client's requests while `reading` is set.
fn serve(
    mut stream: UnixStream,
    globals: &[(u32, &'static str, u32)],
    reading: &AtomicBool,
    seen: &Mutex<Seen>,
    outbox: &Receiver<Vec<u8>>,
) {
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let mut pending: Vec<u8> = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        while let Ok(bytes) = outbox.try_recv() {
            if stream.write_all(&bytes).is_err() {
                return;
            }
        }
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
            let registry = {
                let mut seen = seen.lock().unwrap();
                seen.requests.push((object, opcode));
                seen.registry
            };
            let mut answer = Vec::new();
            match (object, opcode) {
                // wl_display.sync(new callback): done, then the id is free.
                (1, 0) => {
                    let callback = word(8);
                    answer.extend(message(callback, 0, &[Arg::Uint(0)]));
                    answer.extend(message(1, 1, &[Arg::Uint(callback)]));
                }
                // wl_display.get_registry(new registry): every global.
                (1, 1) => {
                    let id = word(8);
                    seen.lock().unwrap().registry = Some(id);
                    for &(name, interface, version) in globals {
                        answer.extend(message(
                            id,
                            0,
                            &[Arg::Uint(name), Arg::Str(interface), Arg::Uint(version)],
                        ));
                    }
                }
                // wl_registry.bind(name, interface, version, new id).
                (id, 0) if Some(id) == registry => {
                    let name = word(8);
                    let length = word(12) as usize;
                    let after = 16 + length.next_multiple_of(4);
                    let new_id = word(after + 4);
                    seen.lock()
                        .unwrap()
                        .bound
                        .entry(name)
                        .or_default()
                        .push(new_id);
                }
                _ => {}
            }
            if stream.write_all(&answer).is_err() {
                return;
            }
            pending.drain(..size);
        }
    }
}
