//! The tracker thread: follows names changing owner and the items' and
//! open menus' signals, runs every call that expects a reply as a future
//! polled alongside them (each read no sooner than its read interval
//! allows), sends the handles' calls, and turns what happens into the
//! model's changes, the subscriber's events and the watcher's signals.
//!
//! It never awaits a reply from the bus outside that one poll: the host's
//! module says why.

use std::future::{Future, poll_fn};
use std::io;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
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
use crate::host::StartError;
use crate::host::shared::{Call, Shared, Work};
use crate::host::startup;
use crate::item::ItemId;
use crate::limits::Limits;
use crate::model::{Due, MenuDue, MenuOutcome, MenuPlace, MenuSource, Outcome, Source};

/// The tracker's thread, stopped and joined when dropped.
pub(super) struct Tracker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Tracker {
    /// Starts following `owners` (every `NameOwnerChanged`, opened before
    /// anything else), then finishes starting the host
    /// ([`startup`]), saying how that went on `started`.
    pub(super) fn spawn(
        connection: Connection,
        owners: MessageStream,
        shared: Arc<Shared>,
        limits: Limits,
        timer: Timer,
        started: SyncSender<Result<(), StartError>>,
    ) -> io::Result<Self> {
        let state = State {
            connection,
            shared: Arc::clone(&shared),
            limits,
            timer,
            started: Some(started),
        };
        let thread = thread::Builder::new()
            .name("status-notifier-tracker".into())
            .spawn(move || zbus::block_on(state.run(owners)))?;
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
    /// The items' match is on the bus: the next stage of starting.
    Items(Box<Result<MessageStream, StartError>>),
    /// The host's names are owned, or could not be.
    Started(Box<Result<(), StartError>>),
    Read(Source, Outcome),
    MenuRead(Box<MenuRead>),
    /// `AboutToShow` for a submenu of the item's menu answered that the
    /// menu needs reading again.
    MenuStale(ItemId),
    /// Nothing to apply: the read's item or menu went while it waited, or
    /// the answer asked for nothing.
    Nothing,
}

/// A read of the open menu at `place`, with the match for its signals made
/// before it, and whether the menu signalled meanwhile.
struct MenuRead {
    source: MenuSource,
    outcome: MenuOutcome,
    place: MenuPlace,
    stream: Option<MessageStream>,
    signalled: bool,
}

type Pending = Pin<Box<dyn Future<Output = Finished> + Send>>;

/// Messages a menu read takes from its stream in one poll: a stream's
/// queue (zbus's default).
const DRAIN_AT_ONCE: usize = 64;

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
    /// Where `Host::start` waits to hear whether the host started, until
    /// it has.
    started: Option<SyncSender<Result<(), StartError>>>,
}

/// The reads each step starts.
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

/// An open menu's signals, followed while it is open: a match on the bus
/// for its owner and object alone, dropped (and so removed from the bus)
/// once the menu is not there any more.
struct Watch {
    menu: MenuPlace,
    stream: MessageStream,
}

/// What the tracker waits on, besides its inbox.
struct Sources {
    pending: FuturesUnordered<Pending>,
    owners: MessageStream,
    /// From the first stage of starting on.
    items: Option<MessageStream>,
    watches: Vec<Watch>,
}

impl State {
    async fn run(mut self, owners: MessageStream) {
        let mut sources = Sources {
            pending: FuturesUnordered::new(),
            owners,
            items: None,
            watches: Vec::new(),
        };
        let connection = self.connection.clone();
        sources.pending.push(Box::pin(async move {
            Finished::Items(Box::new(startup::items(connection).await))
        }));
        // Which source is polled first, turn by turn, so that a busy one
        // cannot starve the others.
        let mut turn = 0_usize;
        loop {
            let step = poll_fn(|cx| {
                turn = turn.wrapping_add(1);
                poll_sources(cx, turn, &self.shared, &mut sources)
            })
            .await;
            let next = match step {
                Step::Stop => break,
                Step::Disconnected => {
                    self.shared.disconnected();
                    break;
                }
                Step::Work(work) => self.work(work, &mut sources.pending).await,
                Step::Finished(Finished::Items(items)) => match *items {
                    Ok(items) => {
                        sources.items = Some(items);
                        let connection = self.connection.clone();
                        sources.pending.push(Box::pin(async move {
                            Finished::Started(Box::new(startup::names(connection).await))
                        }));
                        Next::default()
                    }
                    Err(error) => {
                        self.start_ended(Err(error));
                        break;
                    }
                },
                Step::Finished(Finished::Started(result)) => {
                    let failed = result.is_err();
                    self.start_ended(*result);
                    if failed {
                        break;
                    }
                    Next::default()
                }
                Step::Finished(finished) => self.finished(finished, &mut sources.watches).await,
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
            // A menu closed, or moved away from, is no longer followed. Its
            // match goes from the bus on the connection's own executor;
            // signals that still arrive for it are dropped by the model.
            let open = self.shared.with_model(|m| m.open_menus());
            sources.watches.retain(|w| open.contains(&w.menu));
            for due in next.reads {
                sources.pending.push(self.read(due));
            }
            for due in next.menu_reads {
                sources.pending.push(self.menu_read(due));
            }
        }
        // Ended before it started: `Host::start` hears it.
        if self.started.is_some() {
            let gone = io::Error::from(io::ErrorKind::BrokenPipe);
            self.start_ended(Err(StartError::Connect(zbus::Error::InputOutput(
                Arc::new(gone),
            ))));
        }
    }

    /// Tells `Host::start` how starting ended.
    fn start_ended(&mut self, result: Result<(), StartError>) {
        if let Some(started) = self.started.take() {
            // `Host::start` waits for it; if it has gone, nobody needs it.
            let _sent = started.send(result);
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
    ///
    /// The match for the menu's signals there is made first, so that it is
    /// on the bus before the read's first call: a change after the read is
    /// never missed. When the place is followed already, zbus only counts
    /// one more user of the match, without a call. The read's own stream is
    /// drained while the read waits for its replies (a stream left full
    /// would stall every reply behind it), and a change it shows asks for
    /// one more read; the tracker then keeps the stream, unless it follows
    /// the place already or the menu has gone.
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
            let place = MenuPlace {
                id: source.id.clone(),
                owner: source.owner.clone(),
                path: source.path.clone(),
            };
            // It fails only when the connection is gone, which the end of
            // the owners' stream reports.
            let mut stream = streams::menu_signals(&connection, &place.owner, &place.path)
                .await
                .ok();
            let mut read = pin!(menu::read(connection, timer, source, limits));
            let mut signalled = false;
            let (source, outcome) = poll_fn(|cx| {
                if let Some(stream) = stream.as_mut() {
                    // A queue's worth at a time, so that a menu that floods
                    // cannot keep the read from being polled.
                    let mut drained = 0;
                    while drained < DRAIN_AT_ONCE {
                        match Pin::new(&mut *stream).poll_next(cx) {
                            Poll::Ready(Some(message)) => {
                                signalled |=
                                    message.is_ok_and(|m| streams::menu_signal(&m).is_some());
                                drained += 1;
                            }
                            Poll::Ready(None) | Poll::Pending => break,
                        }
                    }
                    // More may be waiting: this future is polled again soon.
                    if drained == DRAIN_AT_ONCE {
                        cx.waker().wake_by_ref();
                    }
                }
                read.as_mut().poll(cx)
            })
            .await;
            Finished::MenuRead(Box::new(MenuRead {
                source,
                outcome,
                place,
                stream,
                signalled,
            }))
        })
    }

    /// `AboutToShow` for `entry` of the item `id`'s open menu, sent to
    /// wherever the menu is now.
    fn about_to_show(&self, id: ItemId, entry: i32) -> Option<Pending> {
        let peer = self.shared.with_model(|m| m.menu_peer(&id))?;
        let connection = self.connection.clone();
        let timer = self.timer.clone();
        let timeout = self.limits.about_to_show_timeout;
        Some(Box::pin(async move {
            let answer =
                menu::about_to_show(&connection, &timer, &peer.owner, &peer.path, entry, timeout)
                    .await;
            match answer {
                Ok(true) => Finished::MenuStale(id),
                Ok(false) | Err(_) => Finished::Nothing,
            }
        }))
    }

    /// Sends the handles' calls, in order (none waits for a reply), and
    /// starts those that expect one; the reads asked for.
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
    async fn finished(&self, finished: Finished, watches: &mut Vec<Watch>) -> Next {
        match finished {
            Finished::Read(source, outcome) => self.read_done(&source, outcome).await,
            Finished::MenuRead(read) => {
                let MenuRead {
                    source,
                    outcome,
                    place,
                    stream,
                    signalled,
                } = *read;
                let now = Instant::now();
                let done = self
                    .shared
                    .with_model(|m| m.menu_read_done(&source, outcome, now));
                if done.unregistered {
                    self.unregistered(std::slice::from_ref(&source.id), 0).await;
                }
                self.shared.report(done.change, None);
                let mut menu_reads: Vec<MenuDue> = done.next.into_iter().collect();
                if signalled {
                    menu_reads.extend(self.shared.with_model(|m| m.ask_menu(&source.id, now)));
                }
                let open = self.shared.with_model(|m| m.open_menus());
                if let Some(stream) = stream
                    && open.contains(&place)
                    && !watches.iter().any(|w| w.menu == place)
                {
                    watches.push(Watch {
                        menu: place,
                        stream,
                    });
                }
                Next {
                    reads: Vec::new(),
                    menu_reads,
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
            Finished::Items(_) | Finished::Started(_) | Finished::Nothing => Next::default(),
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

    /// Announces the items and the number of hosts that left; signals,
    /// which expect no reply.
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
    sources: &mut Sources,
) -> Poll<Step> {
    if let Poll::Ready(asked) = shared.poll_inbox(cx) {
        return Poll::Ready(asked.map_or(Step::Stop, Step::Work));
    }
    for index in 0..4 {
        let step = match (turn + index) % 4 {
            0 => match Pin::new(&mut sources.pending).poll_next(cx) {
                Poll::Ready(Some(finished)) => Some(Step::Finished(finished)),
                // Nothing in flight; the inbox wakes the tracker for more.
                Poll::Ready(None) | Poll::Pending => None,
            },
            1 => message_step(Pin::new(&mut sources.owners).poll_next(cx), Step::Owner),
            2 => sources
                .items
                .as_mut()
                .and_then(|items| message_step(Pin::new(items).poll_next(cx), Step::Signal)),
            _ => poll_watches(cx, turn, &mut sources.watches),
        };
        if let Some(step) = step {
            return Poll::Ready(step);
        }
    }
    Poll::Pending
}

/// Polls each open menu's signals once, from a different one each turn.
fn poll_watches(cx: &mut Context<'_>, turn: usize, watches: &mut [Watch]) -> Option<Step> {
    let count = watches.len();
    (0..count).find_map(|index| {
        let watch = &mut watches[(turn + index) % count];
        message_step(Pin::new(&mut watch.stream).poll_next(cx), Step::MenuSignal)
    })
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
