//! The thread that follows the portal.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};

use futures_core::Stream;
use zbus::OwnedVariant;
use zbus::message::Message;
use zbus::{Connection, MessageStream};

use crate::error::AppearanceError;
use crate::portal;
use crate::preferences::{Key, Preferences};

/// Which bus the portal is looked for on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    /// The session bus, from `DBUS_SESSION_BUS_ADDRESS`.
    Session,
    /// The bus at this D-Bus address, such as `unix:path=/run/…/bus`.
    Address(String),
}

/// What [`Appearance`] tells its subscriber.
#[derive(Debug)]
pub enum Event {
    /// What the portal says, all of it: when following starts, when a
    /// portal appears on the bus, and when the user changes one of the
    /// settings (a signal that changes nothing is not passed on).
    Preferences(Preferences),
    /// There is no settings portal on the bus: when following starts, or
    /// when the portal went away.
    NoPortal,
    /// A problem; following carries on as well as it can.
    Error(AppearanceError),
}

type Subscriber = Box<dyn Fn(Event) + Send + 'static>;

/// Follows the system's appearance preferences in the settings portal,
/// calling a subscriber with each [`Event`], until dropped.
#[derive(Debug)]
pub struct Appearance {
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

impl Appearance {
    /// Connects to `bus` and starts following the portal on a thread of its
    /// own. `subscriber` is called on that thread, one event at a time:
    /// first with the preferences (or [`Event::NoPortal`]), then at every
    /// change.
    ///
    /// # Errors
    ///
    /// [`AppearanceError::Connect`] when the bus cannot be connected to,
    /// [`AppearanceError::Call`] when it cannot be asked for the portal's
    /// signals, and [`AppearanceError::Thread`] when the thread cannot
    /// start.
    pub fn start(
        bus: Bus,
        subscriber: impl Fn(Event) + Send + 'static,
    ) -> Result<Self, AppearanceError> {
        let (connection, owners, settings) = zbus::block_on(async {
            let connection = match bus {
                Bus::Session => Connection::session().await,
                Bus::Address(address) => {
                    zbus::connection::Builder::address(address.as_str())
                        .build()
                        .await
                }
            }
            .map_err(AppearanceError::Connect)?;
            // The streams first, then the questions: a change in between is
            // seen rather than lost.
            let owners = portal::owner_changes(&connection)
                .await
                .map_err(AppearanceError::Call)?;
            let mut settings = Vec::new();
            for key in Key::ALL {
                // The keys read are in namespaces of their own.
                let changes = portal::setting_changes(&connection, key.namespace());
                settings.push(changes.await.map_err(AppearanceError::Call)?);
            }
            Ok::<_, AppearanceError>((connection, owners, settings))
        })?;
        let stop = Arc::new(Stop::default());
        let thread_stop = Arc::clone(&stop);
        let subscriber: Subscriber = Box::new(subscriber);
        let thread = thread::Builder::new()
            .name("appearance".into())
            .spawn(move || {
                let streams = Streams { owners, settings };
                zbus::block_on(follow(&connection, streams, &thread_stop, &subscriber));
            })
            .map_err(AppearanceError::Thread)?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Appearance {
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
            // A panic in the subscriber ended the thread already.
            let _joined = thread.join();
        }
    }
}

/// Asks the thread to end.
#[derive(Debug, Default)]
struct Stop {
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

struct Streams {
    owners: MessageStream,
    /// One for each namespace read.
    settings: Vec<MessageStream>,
}

/// What woke the thread.
enum Wake {
    Stop,
    Owner(Message),
    Setting(Message),
    /// The connection is gone.
    Closed,
}

async fn follow(
    connection: &Connection,
    mut streams: Streams,
    stop: &Stop,
    subscriber: &Subscriber,
) {
    let mut owner = match portal::owner(connection).await {
        Ok(None) => started(connection).await,
        Ok(owner) => owner,
        Err(error) => {
            subscriber(Event::Error(error));
            None
        }
    };
    let mut said = tell(connection, owner.as_deref(), subscriber).await;
    loop {
        match next(&mut streams, stop).await {
            Wake::Stop => return,
            Wake::Closed => {
                subscriber(Event::NoPortal);
                return;
            }
            Wake::Owner(message) => {
                let Ok((_, _, new)) = message.body().deserialize::<(String, String, String)>()
                else {
                    continue;
                };
                let new = (!new.is_empty()).then_some(new);
                // The portal this thread started is already told.
                if new != owner {
                    owner = new;
                    said = tell(connection, owner.as_deref(), subscriber).await;
                }
            }
            Wake::Setting(message) => {
                changed(&message, owner.as_deref(), &mut said, subscriber);
            }
        }
    }
}

/// Passes on a `SettingChanged` `message` from the portal owned by `owner`
/// when it changes one of the settings read, keeping `said` up to date.
fn changed(
    message: &Message,
    owner: Option<&str>,
    said: &mut Preferences,
    subscriber: &Subscriber,
) {
    let header = message.header();
    let from_owner = header
        .sender()
        .is_some_and(|sender| Some(sender.as_str()) == owner);
    if !from_owner {
        return;
    }
    let Ok((namespace, key, value)) = message
        .body()
        .deserialize::<(String, String, OwnedVariant)>()
    else {
        return;
    };
    let Some(key) = Key::find(&namespace, &key) else {
        return;
    };
    match said.set(key, Some(&value)) {
        Ok(true) => subscriber(Event::Preferences(said.clone())),
        Ok(false) => {}
        Err(error) => subscriber(Event::Error(error)),
    }
}

/// Starts the portal through the bus (D-Bus activation) and returns its
/// owner, or `None` when the bus cannot start it: then a portal that comes
/// later is still noticed through `NameOwnerChanged`.
async fn started(connection: &Connection) -> Option<String> {
    portal::start(connection).await.ok()?;
    portal::owner(connection).await.ok().flatten()
}

/// Tells the subscriber what the portal owned by `owner` says (after what
/// could not be read of it), or that there is none; returns what it said,
/// the defaults for no portal.
async fn tell(
    connection: &Connection,
    owner: Option<&str>,
    subscriber: &Subscriber,
) -> Preferences {
    let Some(owner) = owner else {
        subscriber(Event::NoPortal);
        return Preferences::default();
    };
    let (preferences, errors) = portal::preferences(connection, owner).await;
    for error in errors {
        subscriber(Event::Error(error));
    }
    subscriber(Event::Preferences(preferences.clone()));
    preferences
}

async fn next(streams: &mut Streams, stop: &Stop) -> Wake {
    poll_fn(|cx| {
        *stop.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
        if stop.stopped.load(Ordering::SeqCst) {
            return Poll::Ready(Wake::Stop);
        }
        if let Poll::Ready(message) = poll_message(&mut streams.owners, cx) {
            return Poll::Ready(message.map_or(Wake::Closed, Wake::Owner));
        }
        for settings in &mut streams.settings {
            if let Poll::Ready(message) = poll_message(settings, cx) {
                return Poll::Ready(message.map_or(Wake::Closed, Wake::Setting));
            }
        }
        Poll::Pending
    })
    .await
}

/// The next message of `stream`, skipping any that failed to parse (they
/// are not ours); `None` once the connection is gone.
fn poll_message(stream: &mut MessageStream, cx: &mut Context<'_>) -> Poll<Option<Message>> {
    loop {
        match Pin::new(&mut *stream).poll_next(cx) {
            Poll::Ready(Some(Ok(message))) => return Poll::Ready(Some(message)),
            Poll::Ready(Some(Err(_))) => {}
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        }
    }
}
