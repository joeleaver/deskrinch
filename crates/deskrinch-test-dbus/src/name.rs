//! Waiting for a name to be owned on a bus.

use std::future::poll_fn;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures_core::Stream;
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream};

/// Why [`PrivateBus::wait_for_name`](crate::PrivateBus::wait_for_name)
/// returned without the name being owned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NameError {
    /// Talking to the bus failed: connecting, subscribing or asking.
    #[error("could not ask the bus about {name}")]
    Bus {
        /// The name waited for.
        name: String,
        /// What failed.
        #[source]
        source: zbus::Error,
    },
    /// The bus closed the connection before the name was owned.
    #[error("the bus went away before {name} was owned")]
    Disconnected {
        /// The name waited for.
        name: String,
    },
    /// No one took the name within the patience given.
    #[error("no one owned {name} within {patience:?}")]
    NotOwned {
        /// The name waited for.
        name: String,
        /// How long the wait was.
        patience: Duration,
    },
}

/// Waits until `name` has an owner on the bus at `address`, for at most
/// `patience`.
pub(crate) fn wait_for_name(
    address: &str,
    name: &str,
    patience: Duration,
) -> Result<(), NameError> {
    let bus_error = |source| NameError::Bus {
        name: name.to_owned(),
        source,
    };
    zbus::block_on(async {
        let connection = zbus::connection::Builder::address(address)
            .build()
            .await
            .map_err(bus_error)?;
        // Subscribed first, then asked: a name taken in between is seen as
        // a signal, so it is not missed.
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")
            .interface("org.freedesktop.DBus")
            .member("NameOwnerChanged")
            .arg(0, name)
            .build()
            .map_err(bus_error)?;
        let mut changes = MessageStream::for_match_rule(rule, &connection, None)
            .await
            .map_err(bus_error)?;
        if has_owner(&connection, name).await.map_err(bus_error)? {
            return Ok(());
        }
        let deadline = Deadline::start(patience);
        loop {
            let next = poll_fn(|cx| {
                if deadline.passed(cx.waker()) {
                    return Poll::Ready(None);
                }
                Pin::new(&mut changes).poll_next(cx).map(Some)
            })
            .await;
            match next {
                None => {
                    return Err(NameError::NotOwned {
                        name: name.to_owned(),
                        patience,
                    });
                }
                Some(None) => {
                    return Err(NameError::Disconnected {
                        name: name.to_owned(),
                    });
                }
                Some(Some(Err(source))) => return Err(bus_error(source)),
                Some(Some(Ok(message))) => {
                    let owned = message
                        .body()
                        .deserialize::<(&str, &str, &str)>()
                        .is_ok_and(|(changed, _, new_owner)| {
                            changed == name && !new_owner.is_empty()
                        });
                    if owned {
                        return Ok(());
                    }
                }
            }
        }
    })
}

/// Asks the bus whether `name` has an owner now.
async fn has_owner(connection: &Connection, name: &str) -> zbus::Result<bool> {
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &name,
        )
        .await?;
    reply.body().deserialize()
}

/// A point in time that wakes a task when it passes: a thread waiting on a
/// condition variable, which dropping the deadline ends early.
struct Deadline {
    shared: Arc<(Mutex<DeadlineState>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct DeadlineState {
    passed: bool,
    cancelled: bool,
    waker: Option<Waker>,
}

impl Deadline {
    fn start(patience: Duration) -> Self {
        let shared = Arc::new((Mutex::new(DeadlineState::default()), Condvar::new()));
        let thread = thread::spawn({
            let shared = Arc::clone(&shared);
            move || {
                let (state, cancel) = &*shared;
                let state = state.lock().unwrap_or_else(PoisonError::into_inner);
                let (mut state, _) = cancel
                    .wait_timeout_while(state, patience, |state| !state.cancelled)
                    .unwrap_or_else(PoisonError::into_inner);
                if !state.cancelled {
                    state.passed = true;
                    if let Some(waker) = state.waker.take() {
                        waker.wake();
                    }
                }
            }
        });
        Self {
            shared,
            thread: Some(thread),
        }
    }

    /// Whether the deadline has passed; if not, `waker` is woken when it
    /// does.
    fn passed(&self, waker: &Waker) -> bool {
        let mut state = self.shared.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.passed {
            state.waker = Some(waker.clone());
        }
        state.passed
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        let (state, cancel) = &*self.shared;
        state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cancelled = true;
        cancel.notify_all();
        if let Some(thread) = self.thread.take() {
            // It only waits and wakes; a panic in it already reported itself.
            let _joined = thread.join();
        }
    }
}
