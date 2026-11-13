//! The record of starts (`deskrinch-start-record`), held for the launcher
//! on a thread of its own that does all its file work in turn, and handed
//! to the UI at each change.
//!
//! - At start-up the thread loads the record, before the first show (or,
//!   with `remember-starts` off, deletes it).
//! - [`Starts::record`]: the start thread's, once an application started.
//! - [`Starts::forget`]: the bus's `ForgetStarts`, which waits until the
//!   record is deleted.
//! - [`Starts::remember`]: the UI's, following `[general]`
//!   `remember-starts`: off deletes the record and records nothing until it
//!   is on again, which loads it.
//!
//! One thread, so that a start, a forgetting and a change of the setting
//! are applied to the file and handed to the UI in the order they came: a
//! record written just before a forgetting never reaches the UI after it.
//! Waiting for commands, it wakes for nothing else. A record equal to the
//! last handed over is not handed over again.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use deskrinch_component::{Handover, report};
use deskrinch_desktop_entries::DesktopId;
use deskrinch_start_record::{Loaded, Record, Store, now};
use rinch::prelude::Signal;

/// What the thread is asked to do.
enum Command {
    /// `remember-starts` is now this.
    Remember(bool),
    /// This application was started.
    Record(DesktopId),
    /// Delete the record, then answer.
    Forget(Sender<()>),
}

/// Where the record's thread is asked: clonable, from any thread.
#[derive(Debug, Clone)]
pub(crate) struct Starts {
    commands: Sender<Command>,
}

/// The thread's end, until it is started.
pub(crate) struct Waiting {
    commands: Receiver<Command>,
}

/// The record, handed to the UI.
pub(crate) struct Held {
    record: Arc<Handover<Arc<Record>>>,
}

/// A record's handle and the thread's end: the handle can be given out
/// (to the bus) before the thread runs, and what is asked meanwhile waits.
pub(crate) fn channel() -> (Starts, Waiting) {
    let (commands, receiver) = mpsc::channel();
    (Starts { commands }, Waiting { commands: receiver })
}

impl Starts {
    /// Records a start of `id`, if starts are remembered.
    pub(crate) fn record(&self, id: DesktopId) {
        self.send(Command::Record(id));
    }

    /// Follows `remember-starts`.
    pub(crate) fn remember(&self, on: bool) {
        self.send(Command::Remember(on));
    }

    /// Deletes the record, and returns once it is deleted.
    pub(crate) fn forget(&self) {
        let (done, wait) = mpsc::channel();
        self.send(Command::Forget(done));
        // An error means the thread is gone (reported when it went).
        let _done = wait.recv();
    }

    fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!("the record of starts is gone; the request is dropped");
        }
    }
}

impl Waiting {
    /// Starts the thread on `store` (`None`: no state directory, and
    /// nothing is remembered), with `remember-starts` as `remember` at
    /// start-up.
    pub(crate) fn start(self, store: Option<Store>, remember: bool) -> Held {
        let record: Arc<Handover<Arc<Record>>> = Arc::default();
        let deliver = Arc::clone(&record);
        let spawned = std::thread::Builder::new()
            .name("start-record".into())
            .spawn(move || hold(store, remember, &self.commands, &deliver));
        if let Err(error) = spawned {
            report::error("starting the record of starts", &error);
        }
        Held { record }
    }
}

impl Held {
    /// The UI's signal of the record, on the UI thread: the one loaded at
    /// start-up if it is loaded by then, else empty until it is.
    pub(crate) fn into_signal(self) -> Signal<Arc<Record>> {
        self.record.signal(Arc::default())
    }
}

/// The thread: applies each command to the file, and hands each changed
/// record to the UI.
fn hold(
    store: Option<Store>,
    mut remember: bool,
    commands: &Receiver<Command>,
    deliver: &Handover<Arc<Record>>,
) {
    let Some(store) = store else {
        tracing::warn!("there is no state directory: the applications started are not remembered");
        for command in commands {
            if let Command::Forget(done) = command {
                let _answered = done.send(());
            }
        }
        return;
    };
    let mut held = Arc::new(Record::new());
    let mut hand = |record: Record| {
        if *held != record {
            held = Arc::new(record);
            deliver.deliver(Arc::clone(&held));
        }
    };
    hand(follow(&store, remember));
    for command in commands {
        match command {
            Command::Remember(on) if on == remember => {}
            Command::Remember(on) => {
                remember = on;
                hand(follow(&store, remember));
            }
            Command::Record(id) if remember => match store.record(&id, now()) {
                Ok(record) => {
                    hand(record);
                    tracing::info!("recorded a start of {id}");
                }
                Err(error) => report::error(&format!("recording the start of {id}"), &error),
            },
            Command::Record(_) => {}
            Command::Forget(done) => {
                forget(&store);
                hand(Record::new());
                let _answered = done.send(());
            }
        }
    }
}

/// The record when `remember-starts` is `remember`: loaded, or deleted
/// and empty.
fn follow(store: &Store, remember: bool) -> Record {
    if remember {
        load(store)
    } else {
        forget(store);
        Record::new()
    }
}

/// Loads the record, saying what was found.
fn load(store: &Store) -> Record {
    let path = store.path();
    let path = path.display();
    match store.load() {
        Ok(Loaded::Missing) => {
            tracing::info!("no record of starts yet ({path})");
            Record::new()
        }
        Ok(Loaded::Read { record, dropped }) => {
            if dropped > 0 {
                tracing::warn!("{path}: {dropped} invalid starts left out");
            }
            tracing::info!(
                "the record of starts: {} applications ({path})",
                record.len()
            );
            record
        }
        Ok(Loaded::Damaged { reason }) => {
            tracing::warn!(
                "{path} is not a record of starts ({reason}): starting with none; \
                 the next start replaces it"
            );
            Record::new()
        }
        Ok(Loaded::Newer { version }) => {
            tracing::warn!(
                "{path} is a newer version's record (version {version}): it is left \
                 as it is, and nothing is remembered in this run"
            );
            Record::new()
        }
        Err(error) => {
            report::error("loading the record of starts", &error);
            Record::new()
        }
    }
}

/// Deletes the record, reporting what goes wrong.
fn forget(store: &Store) {
    match store.forget() {
        Ok(true) => tracing::info!("the record of starts is deleted"),
        Ok(false) => tracing::info!("there is no record of starts to delete"),
        Err(error) => report::error("deleting the record of starts", &error),
    }
}
