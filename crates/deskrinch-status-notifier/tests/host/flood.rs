//! A flood of messages on the bus, from a connection of its own, for the
//! tests of what the host does under one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use zbus::message::Flags;
use zbus::{Connection, Message};

use crate::common::{PATIENCE, PrivateBus};

/// Item signals a flood sends. The host drops each as it comes (no item is
/// registered at the flood's object), so the host works through many
/// cheaply, and the flood is still running when the step it tests awaits
/// its reply: a stream left unread fills at once.
pub(crate) const SIGNAL_FLOOD: usize = 20_000;

/// Name changes a flood sends: fewer, as each is applied to the model.
/// Nearly fifty of a stream's queues (64), so a stream left unread fills
/// at once, yet a backlog that a host that keeps reading works through in
/// a fraction of its read timeout. A flood of tens of thousands measures
/// the machine instead: with the machine loaded several times over, the
/// host fell more than its read timeout behind one of 40,000.
pub(crate) const NAME_FLOOD: usize = 3_000;

/// The flood's thread, stopped and joined when dropped.
pub(crate) struct Flood {
    stop: Arc<AtomicBool>,
    sent: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

impl Flood {
    /// Sends `message(n)` for `n` from 0, as fast as the bus takes them,
    /// until `count` are sent or the flood is dropped.
    pub(crate) fn start(
        bus: &PrivateBus,
        count: usize,
        message: impl Fn(usize) -> Message + Send + 'static,
    ) -> Self {
        let connection: Connection = bus.client();
        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicUsize::new(0));
        let thread = {
            let (stop, sent) = (Arc::clone(&stop), Arc::clone(&sent));
            thread::spawn(move || {
                for n in 0..count {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    zbus::block_on(connection.send(&message(n))).unwrap();
                    sent.store(n + 1, Ordering::SeqCst);
                }
            })
        };
        Self {
            stop,
            sent,
            thread: Some(thread),
        }
    }

    /// Waits until `count` have been sent.
    pub(crate) fn wait_for(&self, count: usize) {
        let deadline = Instant::now() + PATIENCE;
        while self.sent.load(Ordering::SeqCst) < count {
            assert!(Instant::now() < deadline, "the flood did not get going");
            thread::yield_now();
        }
    }
}

impl Drop for Flood {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _joined = thread.join();
        }
    }
}

/// An item's `NewTitle`, from the flood's own connection: the host's
/// stream of items' signals takes every one.
pub(crate) fn item_signal(_: usize) -> Message {
    Message::signal(
        "/StatusNotifierItem",
        "org.kde.StatusNotifierItem",
        "NewTitle",
    )
    .build(&())
    .unwrap()
}

/// A name taken, then given up, in turn, without waiting for the bus's
/// answer: each one a `NameOwnerChanged` for every connection that follows
/// them.
pub(crate) fn name_change(n: usize) -> Message {
    let name = format!("org.example.Flood.N{}", n / 2);
    let call = |method| {
        Message::method_call("/org/freedesktop/DBus", method)
            .destination("org.freedesktop.DBus")
            .interface("org.freedesktop.DBus")
            .with_flags(Flags::NO_REPLY_EXPECTED)
    };
    if n.is_multiple_of(2) {
        call("RequestName").build(&(name, 4_u32)).unwrap()
    } else {
        call("ReleaseName").build(&(name,)).unwrap()
    }
}
