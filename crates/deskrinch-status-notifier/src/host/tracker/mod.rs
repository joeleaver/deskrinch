//! The tracker thread: follows names changing owner and the items' and
//! open menus' signals, runs every call that expects a reply as a future
//! polled alongside them (each read no sooner than its read interval
//! allows), sends the handles' calls, and turns what happens into the
//! model's changes, the subscriber's events and the watcher's signals.
//!
//! It never awaits a reply from the bus outside that one poll: the host's
//! module says why.

mod menu_read;
mod sources;

use std::future::poll_fn;
use std::io;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use deskrinch_timer::Timer;
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
use crate::model::{Due, MenuDue, Outcome, Source};
use sources::{Finished, MenuRead, Pending, Sources, Step, Watch, poll_sources};

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
        // Ended before starting finished: `started` goes with `self`, and
        // `Host::start` hears it as `StartError::Ended`.
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
        let timer = self.timer.clone();
        let shared = Arc::clone(&self.shared);
        let limits = self.limits.clone();
        Box::pin(async move {
            if let Some(wait) = wait {
                wait.await;
            }
            let Some(source) = shared.with_model(|m| m.source(&due.id)) else {
                return Finished::Nothing;
            };
            let (source, outcome) = item::read(connection, timer, source, limits).await;
            Finished::Read(source, outcome)
        })
    }

    /// The menu read `due` ([`menu_read::menu_read`]).
    fn menu_read(&self, due: MenuDue) -> Pending {
        menu_read::menu_read(
            self.connection.clone(),
            self.timer.clone(),
            Arc::clone(&self.shared),
            self.limits.clone(),
            due,
        )
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
