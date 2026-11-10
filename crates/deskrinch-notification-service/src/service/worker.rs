//! The worker thread: expires notifications on time and delivers the queued
//! outputs, events to the subscriber and signals to the bus, in order.

use std::mem;
use std::sync::{Arc, PoisonError};

use zbus::Connection;
use zbus::names::BusName;
use zbus::object_server::SignalEmitter;

use crate::dbus::interface::{NotificationsSignals as _, PATH};
use crate::service::Event;
use crate::service::shared::{Output, Shared, Signal};

pub(crate) type Subscriber = Box<dyn Fn(Event) + Send + 'static>;

/// Runs until the service stops, after delivering everything queued.
pub(crate) fn run(shared: &Arc<Shared>, connection: &Connection, subscriber: &Subscriber) {
    let emitter = SignalEmitter::new(connection, PATH);
    loop {
        let Some(batch) = next_batch(shared) else {
            return;
        };
        if batch.iter().any(|o| matches!(o, Output::Signal { .. })) {
            zbus::block_on(replies_sent(connection));
        }
        for output in batch {
            match output {
                Output::Event(event) => subscriber(event),
                Output::Signal { signal, to } => {
                    let Ok(emitter) = &emitter else {
                        continue;
                    };
                    let emitter = match to.as_deref().map(BusName::try_from) {
                        None => emitter.clone(),
                        Some(Ok(to)) => emitter.clone().set_destination(to),
                        // A sender's unique name came from the bus; it is valid.
                        Some(Err(_)) => continue,
                    };
                    // A signal fails to go out only when the connection is
                    // gone, which the name watcher reports as
                    // `Event::Disconnected`; there is nobody to tell else.
                    let _sent = zbus::block_on(emit(&emitter, signal));
                }
            }
        }
    }
}

/// Returns once the replies to every method call already received have been
/// sent, so that no signal about a notification overtakes the `Notify` reply
/// that tells its sender the id (a sender that does not know the id yet
/// would ignore `NotificationClosed` and wait forever).
///
/// The object is served without a task per call, so the object server
/// handles calls, reply included, one at a time in arrival order. A `Ping`
/// to this connection itself arrives after every call received so far, and
/// is answered only once those have been replied to.
async fn replies_sent(connection: &Connection) {
    let Some(own) = connection.unique_name() else {
        return;
    };
    // It fails only when the connection is gone, and then nothing is sent
    // anyway.
    let _pinged = connection
        .call_method(
            Some(own.as_str()),
            PATH,
            Some("org.freedesktop.DBus.Peer"),
            "Ping",
            &(),
        )
        .await;
}

/// Waits for outputs, expiring notifications as their time comes. `None`
/// once the service is stopping and nothing is left to deliver.
fn next_batch(shared: &Shared) -> Option<Vec<Output>> {
    let mut state = shared.lock();
    loop {
        let now = shared.clock.now();
        for change in state.model.expire(now) {
            state.record(change);
        }
        if !state.outbox.is_empty() {
            return Some(mem::take(&mut state.outbox).into());
        }
        if state.stopping {
            return None;
        }
        let wait = state
            .model
            .next_expiry()
            .and_then(|deadline| shared.clock.real_time_until(deadline));
        state = match wait {
            Some(timeout) => {
                shared
                    .wake
                    .wait_timeout(state, timeout)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
            None => shared
                .wake
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner),
        };
    }
}

async fn emit(emitter: &SignalEmitter<'_>, signal: Signal) -> zbus::Result<()> {
    match signal {
        Signal::NotificationClosed { id, reason } => {
            emitter.notification_closed(id.get(), reason.code()).await
        }
        Signal::ActionInvoked { id, key } => emitter.action_invoked(id.get(), &key).await,
        Signal::ActivationToken { id, token } => emitter.activation_token(id.get(), &token).await,
    }
}
