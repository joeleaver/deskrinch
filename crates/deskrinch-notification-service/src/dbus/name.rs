//! Owning `org.freedesktop.Notifications`, and watching for that changing.

use std::future::poll_fn;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};
use std::thread::{self, JoinHandle};

use futures_core::Stream as _;
use zbus::fdo::RequestNameFlags;
use zbus::message::Type;
use zbus::names::MemberName;
use zbus::{Connection, MatchRule, MessageStream};

use crate::dbus::interface::NAME;
use crate::service::Event;
use crate::service::shared::Shared;

/// How to request `org.freedesktop.Notifications`: the daemon's policy for
/// taking the name over, and for giving it up.
///
/// The default asks for the name only if nobody has it, keeps it against
/// any later request, and does not wait in line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NameRequest {
    /// Take the name from its current owner, if that owner allows it
    /// (`DBUS_NAME_FLAG_REPLACE_EXISTING`).
    pub replace_existing: bool,
    /// Let a later request with `replace_existing` take the name from this
    /// service (`DBUS_NAME_FLAG_ALLOW_REPLACEMENT`); the subscriber hears of
    /// it as [`Event::NameLost`].
    pub allow_replacement: bool,
    /// When the name is taken, wait in the bus's queue for it instead of
    /// failing; the subscriber hears [`Event::NameAcquired`] when it comes
    /// (the absence of `DBUS_NAME_FLAG_DO_NOT_QUEUE`).
    pub queue: bool,
}

impl NameRequest {
    pub(crate) fn flags(self) -> RequestNameFlags {
        let mut flags = RequestNameFlags::empty();
        flags.set(RequestNameFlags::REPLACE_EXISTING, self.replace_existing);
        flags.set(RequestNameFlags::ALLOW_REPLACEMENT, self.allow_replacement);
        flags.set(RequestNameFlags::DO_NOT_QUEUE, !self.queue);
        flags
    }
}

/// Whether the service owns the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameStatus {
    /// The service owns the name; notifications reach it.
    Owned,
    /// The service waits in the bus's queue for the name.
    Queued,
}

/// The stream of the bus's signals about the name, opened before the name
/// is requested so that none is missed.
pub(crate) async fn name_signals(connection: &Connection) -> zbus::Result<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.DBus")
        .interface("org.freedesktop.DBus")
        .path("/org/freedesktop/DBus")
        .arg(0, NAME)
        .build()?;
    MessageStream::for_match_rule(rule, connection, None).await
}

/// Asks the watcher to end.
#[derive(Default)]
struct Stop {
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

/// The thread that turns `NameAcquired` and `NameLost` into events, and the
/// end of the connection into [`Event::Disconnected`].
pub(crate) struct Watcher {
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Starts watching `signals`; `owned` is whether the name is owned now.
    pub(crate) fn spawn(
        signals: MessageStream,
        shared: Arc<Shared>,
        owned: bool,
    ) -> io::Result<Self> {
        let stop = Arc::new(Stop::default());
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("notifications-name".into())
            .spawn(move || zbus::block_on(watch(signals, &shared, &thread_stop, owned)))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Watcher {
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
            // The thread only ends by returning; a panic in it already
            // reported itself.
            let _joined = thread.join();
        }
    }
}

async fn watch(mut signals: MessageStream, shared: &Shared, stop: &Stop, mut owned: bool) {
    loop {
        let next = poll_fn(|cx| {
            *stop.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
            if stop.stopped.load(Ordering::SeqCst) {
                return Poll::Ready(None);
            }
            Pin::new(&mut signals).poll_next(cx).map(Some)
        })
        .await;
        let message = match next {
            // Stopped.
            None => return,
            Some(Some(Ok(message))) => message,
            // A message that failed to parse is not one of these signals.
            Some(Some(Err(_))) => continue,
            Some(None) => {
                shared.set_owns_name(false);
                shared.push_event(Event::Disconnected);
                return;
            }
        };
        let header = message.header();
        let acquired = match header.member().map(MemberName::as_str) {
            Some("NameAcquired") => true,
            Some("NameLost") => false,
            _ => continue,
        };
        if message.body().deserialize::<&str>().ok() != Some(NAME) || acquired == owned {
            continue;
        }
        owned = acquired;
        shared.set_owns_name(owned);
        if owned {
            shared.push_event(Event::NameAcquired);
        } else {
            // Another service now shows notifications; the ones shown here
            // are closed (reason 4) so their senders are not left waiting.
            shared.close_all();
            shared.push_event(Event::NameLost);
        }
    }
}
