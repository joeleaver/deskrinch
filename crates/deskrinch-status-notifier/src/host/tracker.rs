//! The tracker thread: follows names changing owner and items' signals,
//! runs every read at once, and turns what happens into the model's
//! changes, the subscriber's events and the watcher's signals.

use std::future::{Future, poll_fn};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};

use futures_core::Stream;
use futures_util::stream::FuturesUnordered;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, Message, MessageStream};

use crate::dbus::watcher::Watcher;
use crate::dbus::{WATCHER_PATH, item, streams};
use crate::host::Event;
use crate::host::shared::Shared;
use crate::item::ItemId;
use crate::limits::Limits;
use crate::model::{Outcome, Target};

/// The tracker's thread, stopped and joined when dropped.
pub(super) struct Tracker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Tracker {
    pub(super) fn spawn(
        connection: Connection,
        owners: MessageStream,
        signals: MessageStream,
        shared: Arc<Shared>,
        limits: Limits,
    ) -> io::Result<Self> {
        let state = State {
            connection,
            shared: Arc::clone(&shared),
            limits,
        };
        let thread = thread::Builder::new()
            .name("status-notifier-tracker".into())
            .spawn(move || zbus::block_on(state.run(owners, signals)))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        self.shared.stop_tracker();
        if let Some(thread) = self.thread.take() {
            // The thread only ends by returning; a panic in it already
            // reported itself.
            let _joined = thread.join();
        }
    }
}

type Read = Pin<Box<dyn Future<Output = (ItemId, Outcome)> + Send>>;

/// What woke the tracker.
enum Step {
    Stop,
    Disconnected,
    Start(Vec<(ItemId, Target)>),
    Done(ItemId, Outcome),
    Owner(Message),
    Signal(Message),
    Nothing,
}

struct State {
    connection: Connection,
    shared: Arc<Shared>,
    limits: Limits,
}

impl State {
    async fn run(self, mut owners: MessageStream, mut signals: MessageStream) {
        let mut reads: FuturesUnordered<Read> = FuturesUnordered::new();
        // Which source is polled first, turn by turn, so that a busy one
        // cannot starve the others.
        let mut turn = 0_usize;
        loop {
            let step = poll_fn(|cx| {
                turn = turn.wrapping_add(1);
                poll_sources(
                    cx,
                    turn,
                    &self.shared,
                    &mut reads,
                    &mut owners,
                    &mut signals,
                )
            })
            .await;
            let started = match step {
                Step::Stop => return,
                Step::Disconnected => {
                    self.shared.report([], Some(Event::Disconnected));
                    return;
                }
                Step::Start(started) => started,
                Step::Done(id, outcome) => self.read_done(&id, outcome).await,
                Step::Owner(message) => self.owner_changed(&message).await,
                Step::Signal(message) => streams::item_signal(&message)
                    .and_then(|(sender, path)| {
                        self.shared.with_model(|m| m.signalled(&sender, &path))
                    })
                    .into_iter()
                    .collect(),
                Step::Nothing => Vec::new(),
            };
            for (id, target) in started {
                reads.push(Box::pin(item::read(
                    self.connection.clone(),
                    id,
                    target,
                    self.limits.clone(),
                )));
            }
        }
    }

    /// Applies a finished read; the next read of the item, if one is due.
    async fn read_done(&self, id: &ItemId, outcome: Outcome) -> Vec<(ItemId, Target)> {
        let done = self.shared.with_model(|m| m.read_done(id, outcome));
        if done.unregistered {
            self.unregistered(std::slice::from_ref(id), 0).await;
        }
        self.shared.report(done.change, None);
        done.next.into_iter().collect()
    }

    /// Applies a `NameOwnerChanged`; the reads it calls for.
    async fn owner_changed(&self, message: &Message) -> Vec<(ItemId, Target)> {
        let Some((name, new_owner)) = streams::owner_change(message) else {
            return Vec::new();
        };
        let change = self
            .shared
            .with_model(|m| m.owner_changed(&name, new_owner.as_deref()));
        self.unregistered(&change.unregistered, change.hosts_left)
            .await;
        self.shared.report(change.changes, None);
        change.reads
    }

    /// Announces the items and the number of hosts that left.
    async fn unregistered(&self, items: &[ItemId], hosts: usize) {
        let Ok(emitter) = SignalEmitter::new(&self.connection, WATCHER_PATH) else {
            return;
        };
        // A signal fails to go out only when the connection is gone, which
        // the end of the owners' stream reports.
        for id in items {
            let _sent = Watcher::status_notifier_item_unregistered(&emitter, id.as_str()).await;
        }
        for _ in 0..hosts {
            let _sent = Watcher::status_notifier_host_unregistered(&emitter).await;
        }
    }
}

/// Polls each source once, starting from a different one each turn; the
/// first that is ready is the step.
fn poll_sources(
    cx: &mut Context<'_>,
    turn: usize,
    shared: &Shared,
    reads: &mut FuturesUnordered<Read>,
    owners: &mut MessageStream,
    signals: &mut MessageStream,
) -> Poll<Step> {
    if let Poll::Ready(asked) = shared.poll_inbox(cx) {
        return Poll::Ready(asked.map_or(Step::Stop, Step::Start));
    }
    for source in 0..3 {
        let step = match (turn + source) % 3 {
            0 => match Pin::new(&mut *reads).poll_next(cx) {
                Poll::Ready(Some((id, outcome))) => Some(Step::Done(id, outcome)),
                // No read in flight; the inbox wakes the tracker for more.
                Poll::Ready(None) | Poll::Pending => None,
            },
            1 => message_step(Pin::new(&mut *owners).poll_next(cx), Step::Owner),
            _ => message_step(Pin::new(&mut *signals).poll_next(cx), Step::Signal),
        };
        if let Some(step) = step {
            return Poll::Ready(step);
        }
    }
    Poll::Pending
}

fn message_step(
    polled: Poll<Option<zbus::Result<Message>>>,
    step: fn(Message) -> Step,
) -> Option<Step> {
    match polled {
        Poll::Ready(Some(Ok(message))) => Some(step(message)),
        // A message that failed to parse is not one this host follows.
        Poll::Ready(Some(Err(_))) => Some(Step::Nothing),
        Poll::Ready(None) => Some(Step::Disconnected),
        Poll::Pending => None,
    }
}
