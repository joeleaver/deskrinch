//! The tracker thread: follows names changing owner and the items' and
//! open menus' signals, runs every read at once (each no sooner than its
//! read interval allows), sends the handles' calls, and turns what happens
//! into the model's changes, the subscriber's events and the watcher's
//! signals.

use std::future::{Future, poll_fn};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use deskrinch_timer::Timer;
use futures_core::Stream;
use futures_util::stream::FuturesUnordered;
use zbus::object_server::SignalEmitter;
use zbus::{Connection, Message, MessageStream};

use crate::dbus::watcher::{self, Watcher};
use crate::dbus::{WATCHER_PATH, item, menu, streams};
use crate::host::shared::{Call, Shared, Work};
use crate::item::ItemId;
use crate::limits::Limits;
use crate::model::{Due, MenuDue, MenuOutcome, MenuSource, Outcome, Source};

/// The tracker's thread, stopped and joined when dropped.
pub(super) struct Tracker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Tracker {
    /// `signals` are the items' and the menus' signals.
    pub(super) fn spawn(
        connection: Connection,
        owners: MessageStream,
        signals: [MessageStream; 2],
        shared: Arc<Shared>,
        limits: Limits,
        timer: Timer,
    ) -> io::Result<Self> {
        let state = State {
            connection,
            shared: Arc::clone(&shared),
            limits,
            timer,
        };
        let [items, menus] = signals;
        let thread = thread::Builder::new()
            .name("status-notifier-tracker".into())
            .spawn(move || zbus::block_on(state.run([owners, items, menus])))?;
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

/// What one of the tracker's calls that expect a reply came to.
enum Finished {
    Read(Source, Outcome),
    MenuRead(MenuSource, MenuOutcome),
    /// `AboutToShow` for a submenu of the item's menu answered that the
    /// menu needs reading again.
    MenuStale(ItemId),
    /// Nothing to apply: the read's item or menu went while it waited, or
    /// the answer asked for nothing.
    Nothing,
}

type Pending = Pin<Box<dyn Future<Output = Finished> + Send>>;

/// What woke the tracker.
enum Step {
    Stop,
    Disconnected,
    Work(Work),
    Finished(Finished),
    Owner(Message),
    Signal(Message),
    MenuSignal(Message),
    Nothing,
}

struct State {
    connection: Connection,
    shared: Arc<Shared>,
    limits: Limits,
    timer: Timer,
}

/// The reads and calls each step starts.
#[derive(Default)]
struct Next {
    reads: Vec<Due>,
    menu_reads: Vec<MenuDue>,
}

impl From<Vec<Due>> for Next {
    fn from(reads: Vec<Due>) -> Self {
        Self {
            reads,
            menu_reads: Vec::new(),
        }
    }
}

impl State {
    /// `streams` are the owners', the items' and the menus' signals.
    async fn run(self, mut streams: [MessageStream; 3]) {
        let mut pending: FuturesUnordered<Pending> = FuturesUnordered::new();
        // Which source is polled first, turn by turn, so that a busy one
        // cannot starve the others.
        let mut turn = 0_usize;
        loop {
            let step = poll_fn(|cx| {
                turn = turn.wrapping_add(1);
                poll_sources(cx, turn, &self.shared, &mut pending, &mut streams)
            })
            .await;
            let next = match step {
                Step::Stop => return,
                Step::Disconnected => {
                    self.shared.disconnected();
                    return;
                }
                Step::Work(work) => self.work(work, &mut pending).await,
                Step::Finished(finished) => self.finished(finished).await,
                Step::Owner(message) => self.owner_changed(&message).await,
                Step::Signal(message) => match streams::item_signal(&message) {
                    Some((sender, path)) => self
                        .shared
                        .with_model(|m| m.signalled(&sender, &path, Instant::now()))
                        .into(),
                    None => Next::default(),
                },
                Step::MenuSignal(message) => match streams::menu_signal(&message) {
                    Some((sender, path)) => Next {
                        reads: Vec::new(),
                        menu_reads: self
                            .shared
                            .with_model(|m| m.menu_signalled(&sender, &path, Instant::now())),
                    },
                    None => Next::default(),
                },
                Step::Nothing => Next::default(),
            };
            for due in next.reads {
                pending.push(self.read(due));
            }
            for due in next.menu_reads {
                pending.push(self.menu_read(due));
            }
        }
    }

    /// The read `due`, once its time has come, from whoever owns the item
    /// then (its owner may change while it waits); nothing when the item is
    /// no longer registered by then.
    fn read(&self, due: Due) -> Pending {
        let wait = (due.at > Instant::now()).then(|| self.timer.sleep_until(due.at));
        let connection = self.connection.clone();
        let shared = Arc::clone(&self.shared);
        let limits = self.limits.clone();
        Box::pin(async move {
            if let Some(wait) = wait {
                wait.await;
            }
            let Some(source) = shared.with_model(|m| m.source(&due.id)) else {
                return Finished::Nothing;
            };
            let (source, outcome) = item::read(connection, source, limits).await;
            Finished::Read(source, outcome)
        })
    }

    /// The menu read `due`, once its time has come, from wherever the menu
    /// is then; nothing when it was closed by then.
    fn menu_read(&self, due: MenuDue) -> Pending {
        let wait = (due.at > Instant::now()).then(|| self.timer.sleep_until(due.at));
        let connection = self.connection.clone();
        let timer = self.timer.clone();
        let shared = Arc::clone(&self.shared);
        let limits = self.limits.clone();
        Box::pin(async move {
            if let Some(wait) = wait {
                wait.await;
            }
            let Some(source) = shared.with_model(|m| m.menu_source(&due.id)) else {
                return Finished::Nothing;
            };
            let (source, outcome) = menu::read(connection, timer, source, limits).await;
            Finished::MenuRead(source, outcome)
        })
    }

    /// `AboutToShow` for `entry` of the item `id`'s open menu, sent to
    /// wherever the menu is now.
    fn about_to_show(&self, id: ItemId, entry: i32) -> Option<Pending> {
        let source = self.shared.with_model(|m| m.menu_peer(&id))?;
        let connection = self.connection.clone();
        let timer = self.timer.clone();
        let timeout = self.limits.about_to_show_timeout;
        Some(Box::pin(async move {
            let answer = menu::about_to_show(
                &connection,
                &timer,
                &source.owner,
                &source.path,
                entry,
                timeout,
            )
            .await;
            match answer {
                Ok(true) => Finished::MenuStale(id),
                Ok(false) | Err(_) => Finished::Nothing,
            }
        }))
    }

    /// Sends the handles' calls, in order, starting those that expect a
    /// reply; the reads asked for.
    async fn work(&self, work: Work, pending: &mut FuturesUnordered<Pending>) -> Next {
        for call in work.calls {
            // Sending fails only when the connection is gone, which the end
            // of the owners' stream reports.
            let _sent = match call {
                Call::Activate {
                    target,
                    activation,
                    token,
                } => item::activate(&self.connection, &target, activation, token.as_deref()).await,
                Call::MenuEvent {
                    target,
                    entry,
                    event,
                    token,
                } => menu::event(&self.connection, &target, entry, event, token.as_deref()).await,
                Call::AboutToShow { id, entry } => {
                    pending.extend(self.about_to_show(id, entry));
                    Ok(())
                }
            };
        }
        Next {
            reads: work.reads,
            menu_reads: work.menu_reads,
        }
    }

    /// Applies what a call that expected a reply came to.
    async fn finished(&self, finished: Finished) -> Next {
        match finished {
            Finished::Read(source, outcome) => self.read_done(&source, outcome).await,
            Finished::MenuRead(source, outcome) => {
                let done = self
                    .shared
                    .with_model(|m| m.menu_read_done(&source, outcome, Instant::now()));
                self.shared.report(done.change, None);
                Next {
                    reads: Vec::new(),
                    menu_reads: done.next.into_iter().collect(),
                }
            }
            Finished::MenuStale(id) => Next {
                reads: Vec::new(),
                menu_reads: self
                    .shared
                    .with_model(|m| m.ask_menu(&id, Instant::now()))
                    .into_iter()
                    .collect(),
            },
            Finished::Nothing => Next::default(),
        }
    }

    /// Applies a finished read; the next reads of the item and its menu.
    async fn read_done(&self, source: &Source, outcome: Outcome) -> Next {
        let done = self
            .shared
            .with_model(|m| m.read_done(&source.id, &source.owner, outcome, Instant::now()));
        if done.unregistered {
            self.unregistered(std::slice::from_ref(&source.id), 0).await;
        }
        self.shared
            .report(done.change.into_iter().chain(done.menu_change), None);
        Next {
            reads: done.next.into_iter().collect(),
            menu_reads: done.menu_next.into_iter().collect(),
        }
    }

    /// Applies a `NameOwnerChanged`; the reads it calls for.
    async fn owner_changed(&self, message: &Message) -> Next {
        let Some((name, new_owner)) = streams::owner_change(message) else {
            return Next::default();
        };
        let change = self
            .shared
            .with_model(|m| m.owner_changed(&name, new_owner.as_deref(), Instant::now()));
        self.unregistered(&change.unregistered, change.hosts_left)
            .await;
        self.shared.report(change.changes, None);
        Next {
            reads: change.reads,
            menu_reads: change.menu_reads,
        }
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
        if !items.is_empty() {
            watcher::items_changed(&self.connection).await;
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
    pending: &mut FuturesUnordered<Pending>,
    streams: &mut [MessageStream; 3],
) -> Poll<Step> {
    if let Poll::Ready(asked) = shared.poll_inbox(cx) {
        return Poll::Ready(asked.map_or(Step::Stop, Step::Work));
    }
    let [owners, items, menus] = streams;
    for index in 0..4 {
        let step = match (turn + index) % 4 {
            0 => match Pin::new(&mut *pending).poll_next(cx) {
                Poll::Ready(Some(finished)) => Some(Step::Finished(finished)),
                // Nothing in flight; the inbox wakes the tracker for more.
                Poll::Ready(None) | Poll::Pending => None,
            },
            1 => message_step(Pin::new(&mut *owners).poll_next(cx), Step::Owner),
            2 => message_step(Pin::new(&mut *items).poll_next(cx), Step::Signal),
            _ => message_step(Pin::new(&mut *menus).poll_next(cx), Step::MenuSignal),
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
