//! A stand-in item on its own connection, answering `GetAll` with whatever
//! the test sets (well-formed or not, or not at all) and recording the
//! calls it receives.
//!
//! It handles its method calls by hand instead of serving an object, so a
//! test can send properties of any type and any size.

use std::collections::HashMap;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use futures_core::Stream as _;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::message::Type;
use zbus::wire::{OwnedVariant, Variant};
use zbus::{Connection, Message, MessageStream};

use crate::common::{PATIENCE, PrivateBus, call_watcher};

/// A property value, from anything zbus can make a variant of.
pub(crate) fn v<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
    value.into().try_into().unwrap()
}

/// The properties of an ordinary item titled `title`.
pub(crate) fn properties(title: &str) -> HashMap<String, OwnedVariant> {
    HashMap::from([
        ("Id".to_owned(), v("stand-in")),
        ("Title".to_owned(), v(title)),
        ("Status".to_owned(), v("Active")),
        ("Category".to_owned(), v("ApplicationStatus")),
        ("IconName".to_owned(), v("mail-unread")),
        (
            "Menu".to_owned(),
            v(zbus::wire::ObjectPath::try_from("/MenuBar").unwrap()),
        ),
    ])
}

/// How the stand-in answers `GetAll`.
#[derive(Debug, Clone)]
pub(crate) enum Reply {
    /// With these properties.
    Properties(HashMap<String, OwnedVariant>),
    /// Not at all.
    Silent,
}

/// A call the item received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    Token(String),
    Activate(i32, i32),
    SecondaryActivate(i32, i32),
    ContextMenu(i32, i32),
    Scroll(i32, String),
}

#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// The stand-in; it leaves the bus when dropped.
pub(crate) struct StandIn {
    connection: Connection,
    path: String,
    reply: Arc<Mutex<Reply>>,
    reads: Arc<Mutex<Vec<Instant>>>,
    calls: Receiver<Call>,
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

impl StandIn {
    /// A stand-in at `path` on `bus`, answering `reply`.
    pub(crate) fn start(bus: &PrivateBus, path: &str, reply: Reply) -> Self {
        let connection = bus.client();
        let reply = Arc::new(Mutex::new(reply));
        let reads = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(Stop::default());
        let (tx, calls) = mpsc::channel();
        let serve = Serve {
            connection: connection.clone(),
            path: path.to_owned(),
            reply: Arc::clone(&reply),
            reads: Arc::clone(&reads),
            calls: tx,
            stop: Arc::clone(&stop),
        };
        let stream = MessageStream::from(&connection);
        let thread = thread::spawn(move || zbus::block_on(serve.run(stream)));
        Self {
            connection,
            path: path.to_owned(),
            reply,
            reads,
            calls,
            stop,
            thread: Some(thread),
        }
    }

    /// Its connection's unique name.
    pub(crate) fn unique_name(&self) -> String {
        self.connection.unique_name().unwrap().to_string()
    }

    /// Owns the well-known `name`.
    pub(crate) fn own(&self, name: &str) {
        zbus::block_on(self.connection.request_name(name)).unwrap();
    }

    /// Owns the well-known `name`, letting another connection take it over.
    pub(crate) fn own_replaceably(&self, name: &str) {
        let flags = RequestNameFlags::ALLOW_REPLACEMENT | RequestNameFlags::DO_NOT_QUEUE;
        zbus::block_on(self.connection.request_name_with_flags(name, flags)).unwrap();
    }

    /// Takes the well-known `name` over from its owner.
    pub(crate) fn take_over(&self, name: &str) {
        let flags = RequestNameFlags::REPLACE_EXISTING | RequestNameFlags::DO_NOT_QUEUE;
        let reply = zbus::block_on(self.connection.request_name_with_flags(name, flags));
        assert_eq!(reply.unwrap(), RequestNameReply::PrimaryOwner);
    }

    /// How many times its properties were read.
    pub(crate) fn reads(&self) -> usize {
        self.read_times().len()
    }

    /// When each read of its properties arrived, in order.
    pub(crate) fn read_times(&self) -> Vec<Instant> {
        self.reads.lock().unwrap().clone()
    }

    /// Registers with the watcher, passing `service`.
    pub(crate) fn register(&self, service: &str) -> zbus::Result<()> {
        call_watcher(&self.connection, "RegisterStatusNotifierItem", service)
    }

    /// Answers `reply` from now on.
    pub(crate) fn set(&self, reply: Reply) {
        *self.reply.lock().unwrap() = reply;
    }

    /// Sends the item signal `member` (`NewTitle`, say).
    pub(crate) fn signal(&self, member: &str) {
        zbus::block_on(self.connection.emit_signal(
            None::<&str>,
            self.path.as_str(),
            "org.kde.StatusNotifierItem",
            member,
            &(),
        ))
        .unwrap();
    }

    /// The next call it received.
    pub(crate) fn next_call(&self) -> Call {
        self.calls
            .recv_timeout(PATIENCE)
            .expect("the item received no call in time")
    }

    /// That no call arrives for a moment.
    pub(crate) fn no_call(&self) {
        if let Ok(call) = self.calls.recv_timeout(crate::common::QUIET) {
            panic!("unexpected call {call:?}");
        }
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop.stopped.store(true, Ordering::SeqCst);
        let waker = self
            .stop
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(waker) = waker {
            waker.wake();
        }
        if let Some(thread) = self.thread.take() {
            let _joined = thread.join();
        }
        let _closed = zbus::block_on(self.connection.clone().close());
    }
}

struct Serve {
    connection: Connection,
    path: String,
    reply: Arc<Mutex<Reply>>,
    reads: Arc<Mutex<Vec<Instant>>>,
    calls: Sender<Call>,
    stop: Arc<Stop>,
}

impl Serve {
    async fn run(self, mut stream: MessageStream) {
        loop {
            let next = poll_fn(|cx| {
                *self
                    .stop
                    .waker
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
                if self.stop.stopped.load(Ordering::SeqCst) {
                    return Poll::Ready(None);
                }
                Pin::new(&mut stream).poll_next(cx).map(Some)
            })
            .await;
            match next {
                Some(Some(Ok(message))) => self.handle(&message).await,
                Some(Some(Err(_))) => {}
                None | Some(None) => return,
            }
        }
    }

    async fn handle(&self, message: &Message) {
        let header = message.header();
        if header.message_type() != Type::MethodCall
            || header.path().map(zbus::ObjectPath::as_str) != Some(self.path.as_str())
        {
            return;
        }
        let interface = header.interface().map(ToString::to_string);
        let member = header.member().map(ToString::to_string).unwrap_or_default();
        let body = message.body();
        let call = match (interface.as_deref(), member.as_str()) {
            (Some("org.freedesktop.DBus.Properties"), "GetAll") => {
                self.reads.lock().unwrap().push(Instant::now());
                let reply = self.reply.lock().unwrap().clone();
                if let Reply::Properties(properties) = reply {
                    let message = Message::method_return(&header).build(&properties).unwrap();
                    self.connection.send(&message).await.unwrap();
                }
                return;
            }
            (_, "ProvideXdgActivationToken") => Call::Token(body.deserialize().unwrap()),
            (_, "Activate") => {
                let (x, y) = body.deserialize().unwrap();
                Call::Activate(x, y)
            }
            (_, "SecondaryActivate") => {
                let (x, y) = body.deserialize().unwrap();
                Call::SecondaryActivate(x, y)
            }
            (_, "ContextMenu") => {
                let (x, y) = body.deserialize().unwrap();
                Call::ContextMenu(x, y)
            }
            (_, "Scroll") => {
                let (delta, orientation) = body.deserialize().unwrap();
                Call::Scroll(delta, orientation)
            }
            _ => return,
        };
        let _sent = self.calls.send(call);
    }
}
