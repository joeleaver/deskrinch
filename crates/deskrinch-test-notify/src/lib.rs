//! A client of `org.freedesktop.Notifications` for deskrinch's tests: the
//! service crate's, which talk to a service on a private bus, and the
//! notification daemon's, which talk to the daemon on a session's bus.
//!
//! - [`Client`]: notifications sent, replaced and closed, on a connection
//!   the test makes, with the signals that connection receives.
//! - [`Signals`]: the interface's signals a connection receives, in order:
//!   with the service's default, sender-only delivery, the ones about the
//!   notifications that connection sent.
//!
//! # Example
//!
//! ```no_run
//! use deskrinch_test_notify::{Client, Signal};
//!
//! let connection = zbus::block_on(zbus::Connection::session())?;
//! let client = Client::on(&connection);
//! let id = client.simple(0, "Hello", "World", 1000);
//! assert_eq!(client.signals.next(), Signal::Closed(id, 1));
//! # Ok::<(), zbus::Error>(())
//! ```

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use deskrinch_test_wait::{quiet_for, recv};
use futures_core::Stream as _;
use zbus::message::Type;
use zbus::{Connection, MatchRule, MessageStream, Variant};

/// The interface's methods, as the specification gives them.
#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
pub trait Notifications {
    /// `Notify`.
    #[expect(clippy::too_many_arguments, reason = "the specification's method")]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    /// `CloseNotification`.
    fn close_notification(&self, id: u32) -> zbus::Result<()>;

    /// `GetCapabilities`.
    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

    /// `GetServerInformation`.
    fn get_server_information(&self) -> zbus::Result<(String, String, String, String)>;
}

/// A client of the service, and the signals its own connection receives.
pub struct Client {
    /// The interface on the connection.
    pub proxy: NotificationsProxy<'static>,
    /// The signals the connection receives, from when the client was made.
    pub signals: Signals,
}

impl Client {
    /// A client on `connection`.
    ///
    /// # Panics
    ///
    /// When the connection cannot listen for the signals or make the proxy.
    #[must_use]
    pub fn on(connection: &Connection) -> Self {
        let signals = Signals::on(connection);
        let proxy = zbus::block_on(NotificationsProxy::new(connection))
            .unwrap_or_else(|why| panic!("the notifications interface on the connection: {why}"));
        Self { proxy, signals }
    }

    /// Sends a notification with no actions or hints, from "Test".
    ///
    /// # Panics
    ///
    /// When the service refuses it.
    #[expect(
        clippy::must_use_candidate,
        reason = "a test may send a notification only to have it shown, and need no id"
    )]
    pub fn simple(&self, replaces_id: u32, summary: &str, body: &str, timeout: i32) -> u32 {
        self.send(replaces_id, summary, body, &[], &HashMap::new(), timeout)
            .unwrap_or_else(|why| panic!("the service refused a notification: {why}"))
    }

    /// Sends a notification from "Test", with no icon.
    ///
    /// # Errors
    ///
    /// The service's refusal, or the bus's failure.
    pub fn send(
        &self,
        replaces_id: u32,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: &HashMap<&str, Variant<'_>>,
        timeout: i32,
    ) -> zbus::Result<u32> {
        zbus::block_on(self.proxy.notify(
            "Test",
            replaces_id,
            "",
            summary,
            body,
            actions,
            hints,
            timeout,
        ))
    }

    /// Closes notification `id`, as its sender.
    ///
    /// # Errors
    ///
    /// The service's refusal, or the bus's failure.
    pub fn close(&self, id: u32) -> zbus::Result<()> {
        zbus::block_on(self.proxy.close_notification(id))
    }
}

/// A signal of the interface, as a client saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// `NotificationClosed`: the id and the reason's code.
    Closed(u32, u32),
    /// `ActionInvoked`: the id and the action's key.
    ActionInvoked(u32, String),
    /// `ActivationToken`: the id and the token.
    ActivationToken(u32, String),
}

/// The interface's signals a connection receives, in order.
pub struct Signals(Receiver<Signal>);

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Signals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signals").finish_non_exhaustive()
    }
}

impl Signals {
    /// Starts listening on `connection`; signals it receives after this
    /// returns are all seen.
    ///
    /// # Panics
    ///
    /// When the connection cannot listen.
    #[must_use]
    pub fn on(connection: &Connection) -> Self {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.Notifications")
            .build()
            .unwrap_or_else(|why| panic!("the signals' match rule: {why}"));
        let mut stream = zbus::block_on(MessageStream::for_match_rule(rule, connection, None))
            .unwrap_or_else(|why| panic!("listening for the signals: {why}"));
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            zbus::block_on(async move {
                while let Some(Ok(message)) =
                    std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx)).await
                {
                    let header = message.header();
                    let body = message.body();
                    let signal = match header.member().map(zbus::names::MemberName::as_str) {
                        Some("NotificationClosed") => {
                            let Ok((id, reason)) = body.deserialize() else {
                                continue;
                            };
                            Signal::Closed(id, reason)
                        }
                        Some("ActionInvoked") => {
                            let Ok((id, key)) = body.deserialize() else {
                                continue;
                            };
                            Signal::ActionInvoked(id, key)
                        }
                        Some("ActivationToken") => {
                            let Ok((id, token)) = body.deserialize() else {
                                continue;
                            };
                            Signal::ActivationToken(id, token)
                        }
                        _ => continue,
                    };
                    if tx.send(signal).is_err() {
                        return;
                    }
                }
            });
        });
        Self(rx)
    }

    /// The next signal, within the patience.
    ///
    /// # Panics
    ///
    /// When none comes.
    #[must_use]
    pub fn next(&self) -> Signal {
        recv("a signal", &self.0)
    }

    /// Checks that no signal comes for `wait`.
    ///
    /// # Panics
    ///
    /// When one does.
    #[track_caller]
    pub fn none_within(&self, wait: Duration) {
        quiet_for("no signal", &self.0, wait);
    }
}
