//! The `org.freedesktop.Notifications` service, and the model of the live
//! notifications behind it.
//!
//! This crate implements version 1.3 of the [Desktop Notifications
//! Specification] as a D-Bus service at `/org/freedesktop/Notifications`. It
//! has no UI: the popups are built on top of it, observing the model and
//! acting on it through a [`Handle`].
//!
//! # The model
//!
//! The model is the list of live [`Notification`]s, in the order they were
//! first shown. Each one goes through this lifecycle, and the subscriber hears
//! of every step as an [`Event`], in order:
//!
//! - **Shown** ([`Event::Shown`]): a `Notify` call made it. It gets a fresh
//!   [`NotificationId`] and goes after the others.
//! - **Replaced** ([`Event::Replaced`]): a `Notify` call named it in
//!   `replaces_id`. It keeps its id and its place; its content and its timer
//!   start over. Naming an id that is not live shows a new notification
//!   with that id: the specification says "If `replaces_id` is not 0, the
//!   returned value is the same value as `replaces_id`." Which senders may
//!   replace or close a notification is [`Config::ownership`].
//! - **Closed** ([`Event::Closed`]), once, with a [`CloseReason`]:
//!   - [`Expired`](CloseReason::Expired): its timeout passed. A timeout of
//!     `-1` is [`Config::default_timeout`], `0` is never, a positive one is
//!     that many milliseconds; a critical notification never expires.
//!   - [`Dismissed`](CloseReason::Dismissed): the user dismissed it
//!     ([`Handle::dismiss`]), or invoked an action on it
//!     ([`Handle::invoke_action`]) and it is not resident.
//!   - [`Closed`](CloseReason::Closed): the sender called
//!     `CloseNotification`.
//!
//! The bus hears the same steps through `NotificationClosed`, and actions
//! through `ActivationToken` and `ActionInvoked`; by default only the
//! notification's current sender receives them ([`Config::signal_delivery`]).
//! `GetCapabilities` reports [`Capabilities`], which the model follows.
//!
//! # Untrusted input
//!
//! Every sender is untrusted. Before a notification enters the model, its
//! strings are cleaned of control characters and bounded, its lists are
//! bounded, and each sender may hold only so many live notifications
//! ([`Limits`]). The body's markup is parsed into a [`Body`] of styled
//! [`Span`]s, so the UI never interprets markup (see [`Body`] for exactly
//! what is kept, escaped or dropped). Pixel data is validated and decoded into
//! an [`Image`] of plain RGBA. Files named by the sender are only
//! [`ImageSource::File`] paths: this crate never reads them.
//!
//! # Threads
//!
//! No async runtime is needed. The service runs on threads of its own;
//! the subscriber is called on one of them, never with a lock held, so it may
//! call back into a [`Handle`]. A UI forwards each event to its own thread.
//! Time comes from a [`Clock`]: [`SystemClock`] in a daemon, [`ManualClock`]
//! in tests, where expiry happens without sleeping.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use deskrinch_notification_service::{Config, Event, Service, SystemClock};
//!
//! let service = Service::start(Config::default(), Arc::new(SystemClock), |event| match event {
//!     Event::Shown(n) | Event::Replaced(n) => println!("{}: {}", n.id, n.summary),
//!     Event::Closed { id, reason } => println!("{id} closed: {reason:?}"),
//!     _ => {}
//! })?;
//! let handle = service.handle();
//! for notification in handle.notifications() {
//!     if notification.default_action().is_some() {
//!         handle.invoke_action(notification.id, "default", None)?;
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! [Desktop Notifications Specification]: https://specifications.freedesktop.org/notification/1.3/

mod capabilities;
mod clock;
mod dbus;
mod image;
mod limits;
mod markup;
mod model;
mod notification;
mod policy;
#[cfg(test)]
mod props;
mod request;
mod service;
mod text;

pub use capabilities::Capabilities;
pub use clock::{Clock, ManualClock, SystemClock};
pub use dbus::name::{NameRequest, NameStatus};
pub use image::{IconName, Image, ImageSource};
pub use limits::Limits;
pub use markup::{Body, Link, Span, Style};
pub use notification::{Action, CloseReason, Expiry, Notification, NotificationId, Sound, Urgency};
pub use policy::{Ownership, SignalDelivery};
pub use service::{Bus, Config, Event, Handle, OperationError, ServerInfo, Service, StartError};
