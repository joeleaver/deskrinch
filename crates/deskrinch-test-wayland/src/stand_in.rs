//! A stand-in compositor on one end of a socket pair, for what a real one
//! cannot be made to do on cue: stop reading, send given events in one
//! write, or announce an object after the client asked it to stop.
//!
//! It answers `wl_display.sync`, lists its globals on
//! `wl_display.get_registry`, notes the id each `wl_registry.bind` gives a
//! global, notes every request it reads by its object and opcode, and
//! answers nothing else. A test writes events of its own with [`message`]
//! and [`StandIn::send`]; objects it creates take ids from [`SERVER_ID`] up,
//! as a compositor's do.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use wayland_client::{ConnectError, Connection};

use crate::Deadline;

/// The first id a compositor gives an object it creates (an event's
/// `new_id`).
pub const SERVER_ID: u32 = 0xff00_0000;

/// How long a read waits before the stand-in looks at what to send.
const POLL: Duration = Duration::from_millis(20);

/// An argument of a message.
#[derive(Debug, Clone, Copy)]
pub enum Arg<'a> {
    /// A `uint`, `int`, object id or `new_id`.
    Uint(u32),
    /// A `string`.
    Str(&'a str),
}

/// The bytes of message `opcode` from (or to) `object`. A string longer
/// than a message can hold is cut to what fits; no test sends one.
#[must_use]
pub fn message(object: u32, opcode: u16, args: &[Arg<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for arg in args {
        match *arg {
            Arg::Uint(value) => body.extend_from_slice(&value.to_ne_bytes()),
            Arg::Str(text) => {
                let length = u32::try_from(text.len() + 1).unwrap_or(u32::MAX);
                body.extend_from_slice(&length.to_ne_bytes());
                body.extend_from_slice(text.as_bytes());
                body.push(0);
                while body.len() % 4 != 0 {
                    body.push(0);
                }
            }
        }
    }
    let size = u32::try_from(8 + body.len()).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(8 + body.len());
    bytes.extend_from_slice(&object.to_ne_bytes());
    bytes.extend_from_slice(&((size << 16) | u32::from(opcode)).to_ne_bytes());
    bytes.extend_from_slice(&body);
    bytes
}

/// Why a stand-in could not be started.
#[derive(Debug, thiserror::Error)]
pub enum StandInError {
    /// The socket pair could not be made.
    #[error("could not make a socket pair")]
    Socket(#[source] std::io::Error),
    /// The client's end could not be made a connection.
    #[error("could not make a connection on the socket pair")]
    Connect(#[source] ConnectError),
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

/// A global the stand-in offers: its name, interface and version.
pub type Offered = (u32, &'static str, u32);

/// The stand-in compositor, as a test drives it.
#[derive(Debug)]
pub struct StandIn {
    reading: Arc<AtomicBool>,
    seen: Arc<Mutex<Seen>>,
    outbox: Sender<Vec<u8>>,
}

impl StandIn {
    /// Starts a stand-in offering `globals`, and a connection to it.
    ///
    /// # Errors
    ///
    /// When the socket pair or the connection cannot be made.
    pub fn start(globals: &[Offered]) -> Result<(Connection, Self), StandInError> {
        let (ours, theirs) = UnixStream::pair().map_err(StandInError::Socket)?;
        let (outbox, sent) = mpsc::channel();
        let stand_in = Self {
            reading: Arc::new(AtomicBool::new(true)),
            seen: Arc::default(),
            outbox,
        };
        let reading = Arc::clone(&stand_in.reading);
        let seen = Arc::clone(&stand_in.seen);
        let globals = globals.to_vec();
        thread::spawn(move || serve(theirs, &globals, &reading, &seen, &sent));
        let connection = Connection::from_socket(ours).map_err(StandInError::Connect)?;
        Ok((connection, stand_in))
    }

    /// Whether it reads what the client sends.
    pub fn set_reading(&self, reading: bool) {
        self.reading.store(reading, Ordering::Release);
    }

    /// The client's `wl_registry`'s id, once it has asked for one.
    #[must_use]
    pub fn registry(&self) -> Option<u32> {
        lock(&self.seen).registry
    }

    /// The ids the client gave the global named `name`, one per bind.
    #[must_use]
    pub fn bound(&self, name: u32) -> Vec<u32> {
        lock(&self.seen)
            .bound
            .get(&name)
            .cloned()
            .unwrap_or_default()
    }

    /// The opcodes of the requests read so far on `object`, in order.
    #[must_use]
    pub fn requests_to(&self, object: u32) -> Vec<u32> {
        lock(&self.seen)
            .requests
            .iter()
            .filter(|&&(to, _)| to == object)
            .map(|&(_, opcode)| opcode)
            .collect()
    }

    /// Waits until request `opcode` has been read on `object`; whether it
    /// was before `deadline`.
    #[must_use]
    pub fn wait_request(&self, object: u32, opcode: u32, deadline: Deadline) -> bool {
        loop {
            if self.requests_to(object).contains(&opcode) {
                return true;
            }
            if deadline.left().is_zero() {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Writes `bytes` to the client, in one write. Nothing happens once the
    /// client has closed the connection.
    pub fn send(&self, bytes: Vec<u8>) {
        // The stand-in's thread is gone only once the client is.
        drop(self.outbox.send(bytes));
    }
}

/// Locks `seen`, through a panic of another holder: plain values, valid at
/// every step.
fn lock(seen: &Mutex<Seen>) -> MutexGuard<'_, Seen> {
    seen.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The `u32` at byte `at` of `bytes`.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let chunk = bytes.get(at..at + 4)?;
    Some(u32::from_ne_bytes(chunk.try_into().ok()?))
}

/// The stand-in's thread: writes what the test sends, and reads and
/// answers the client's requests while `reading` is set. It ends when the
/// client closes the connection.
fn serve(
    mut stream: UnixStream,
    globals: &[Offered],
    reading: &AtomicBool,
    seen: &Mutex<Seen>,
    outbox: &Receiver<Vec<u8>>,
) {
    if stream.set_read_timeout(Some(POLL)).is_err() {
        return;
    }
    let mut pending: Vec<u8> = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        while let Ok(bytes) = outbox.try_recv() {
            if stream.write_all(&bytes).is_err() {
                return;
            }
        }
        if !reading.load(Ordering::Acquire) {
            thread::sleep(POLL);
            continue;
        }
        match stream.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => pending.extend_from_slice(&buffer[..read]),
            Err(_) => continue,
        }
        // Whole messages: object, then size (high half) and opcode.
        while let (Some(object), Some(header)) = (word(&pending, 0), word(&pending, 4)) {
            let (size, opcode) = ((header >> 16) as usize, header & 0xffff);
            if size < 8 || pending.len() < size {
                break;
            }
            let answer = answer(&pending[..size], object, opcode, globals, seen);
            if stream.write_all(&answer).is_err() {
                return;
            }
            pending.drain(..size);
        }
    }
}

/// Notes request `opcode` on `object` (the whole message in `request`),
/// and what the stand-in sends back for it.
fn answer(
    request: &[u8],
    object: u32,
    opcode: u32,
    globals: &[Offered],
    seen: &Mutex<Seen>,
) -> Vec<u8> {
    let registry = {
        let mut seen = lock(seen);
        seen.requests.push((object, opcode));
        seen.registry
    };
    let mut answer = Vec::new();
    match (object, opcode) {
        // wl_display.sync(new callback): done, then the id is free.
        (1, 0) => {
            if let Some(callback) = word(request, 8) {
                answer.extend(message(callback, 0, &[Arg::Uint(0)]));
                answer.extend(message(1, 1, &[Arg::Uint(callback)]));
            }
        }
        // wl_display.get_registry(new registry): every global.
        (1, 1) => {
            if let Some(id) = word(request, 8) {
                lock(seen).registry = Some(id);
                for &(name, interface, version) in globals {
                    answer.extend(message(
                        id,
                        0,
                        &[Arg::Uint(name), Arg::Str(interface), Arg::Uint(version)],
                    ));
                }
            }
        }
        // wl_registry.bind(name, interface, version, new id).
        (id, 0) if Some(id) == registry => {
            let bound = word(request, 8)
                .zip(word(request, 12))
                .and_then(|(name, length)| {
                    let after = 16 + (length as usize).next_multiple_of(4);
                    Some((name, word(request, after + 4)?))
                });
            if let Some((name, new_id)) = bound {
                lock(seen).bound.entry(name).or_default().push(new_id);
            }
        }
        _ => {}
    }
    answer
}
