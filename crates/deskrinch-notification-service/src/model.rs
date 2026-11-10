//! The live notifications and their lifecycle, with no I/O: every operation
//! takes the time it happens at and returns the changes it made.

use std::sync::Arc;
use std::time::Instant;

use crate::notification::{CloseReason, Expiry, Notification, NotificationId};
use crate::request::{Policy, Request};

/// A change to the model, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Change {
    /// A new notification is live, after the others.
    Shown(Arc<Notification>),
    /// A live notification's content was replaced in place.
    Replaced(Arc<Notification>),
    /// A notification left the model.
    Closed {
        id: NotificationId,
        reason: CloseReason,
    },
    /// The user invoked an action.
    ActionInvoked {
        id: NotificationId,
        key: String,
        activation_token: Option<String>,
    },
}

/// Why a `Notify` call made no notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Refused {
    #[error("{0} notifications are live, the most the server keeps")]
    TooMany(usize),
    #[error("this sender has {0} live notifications, the most one sender may have")]
    TooManyFromSender(usize),
}

/// The live notifications, in the order they were first shown.
#[derive(Debug)]
pub(crate) struct Model {
    policy: Policy,
    live: Vec<Arc<Notification>>,
    next_id: u32,
}

impl Model {
    pub(crate) fn new(policy: Policy) -> Self {
        Self {
            policy,
            live: Vec::new(),
            next_id: 1,
        }
    }

    pub(crate) fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The live notifications, in order.
    pub(crate) fn live(&self) -> &[Arc<Notification>] {
        &self.live
    }

    pub(crate) fn get(&self, id: NotificationId) -> Option<&Arc<Notification>> {
        self.live.iter().find(|n| n.id == id)
    }

    fn position(&self, id: NotificationId) -> Option<usize> {
        self.live.iter().position(|n| n.id == id)
    }

    /// Shows `request` at `now`: replaces the live notification it names, or
    /// adds a new one (also when the one it names is no longer live).
    pub(crate) fn notify(
        &mut self,
        request: &Request<'_>,
        now: Instant,
    ) -> Result<(NotificationId, Change), Refused> {
        let replacing = NotificationId::new(request.replaces_id).and_then(|id| self.position(id));
        let limits = &self.policy.limits;
        let owner = replacing.and_then(|at| self.live[at].sender.as_deref());
        if replacing.is_none() && self.live.len() >= limits.max_live {
            return Err(Refused::TooMany(limits.max_live));
        }
        let moves_to_sender = replacing.is_none() || owner != request.sender.as_deref();
        let from_sender = self
            .live
            .iter()
            .filter(|n| n.sender == request.sender)
            .count();
        if moves_to_sender && from_sender >= limits.max_live_per_sender {
            return Err(Refused::TooManyFromSender(limits.max_live_per_sender));
        }

        let held: usize = self
            .live
            .iter()
            .enumerate()
            .filter(|(at, _)| Some(*at) != replacing)
            .map(|(_, n)| n.image_bytes())
            .sum();
        let budget = limits.max_image_bytes_total.saturating_sub(held);
        if let Some(at) = replacing {
            let id = self.live[at].id;
            let mut built = request.build(id, now, &self.policy, budget);
            // A notification the UI holds paused (under the pointer, say)
            // stays paused with its new content, its full timeout left.
            if matches!(self.live[at].expiry, Expiry::Paused { .. }) {
                built.expiry = built.expiry.paused(now);
            }
            let notification = Arc::new(built);
            self.live[at] = Arc::clone(&notification);
            return Ok((id, Change::Replaced(notification)));
        }
        let id = self.allocate_id();
        let notification = Arc::new(request.build(id, now, &self.policy, budget));
        self.live.push(Arc::clone(&notification));
        Ok((id, Change::Shown(notification)))
    }

    /// The next id from the counter that is not live. Ids wrap from
    /// `u32::MAX` back to 1, never 0.
    fn allocate_id(&mut self) -> NotificationId {
        loop {
            let candidate = NotificationId::new(self.next_id);
            self.next_id = self.next_id.checked_add(1).unwrap_or(1);
            if let Some(id) = candidate.filter(|id| self.position(*id).is_none()) {
                return id;
            }
        }
    }

    /// Removes `id` for `reason`. `None` when it is not live.
    pub(crate) fn close(&mut self, id: NotificationId, reason: CloseReason) -> Option<Change> {
        let at = self.position(id)?;
        self.live.remove(at);
        Some(Change::Closed { id, reason })
    }

    /// The user invoked the action `key` on `id`. Unless the notification is
    /// resident, invoking an action also dismisses it.
    pub(crate) fn invoke_action(
        &mut self,
        id: NotificationId,
        key: &str,
        activation_token: Option<String>,
    ) -> Result<Vec<Change>, ActionRefused> {
        let notification = self.get(id).ok_or(ActionRefused::NotFound)?;
        let action = notification
            .action(key)
            .ok_or(ActionRefused::UnknownAction)?;
        let mut changes = vec![Change::ActionInvoked {
            id,
            key: action.key.clone(),
            activation_token,
        }];
        if !notification.resident {
            changes.extend(self.close(id, CloseReason::Dismissed));
        }
        Ok(changes)
    }

    /// Removes every notification whose expiry is at or before `now`.
    pub(crate) fn expire(&mut self, now: Instant) -> Vec<Change> {
        let mut changes = Vec::new();
        self.live.retain(|n| {
            let expired = matches!(n.expiry, Expiry::At(at) if at <= now);
            if expired {
                changes.push(Change::Closed {
                    id: n.id,
                    reason: CloseReason::Expired,
                });
            }
            !expired
        });
        changes
    }

    /// The earliest expiry among the live notifications.
    pub(crate) fn next_expiry(&self) -> Option<Instant> {
        self.live
            .iter()
            .filter_map(|n| match n.expiry {
                Expiry::At(at) => Some(at),
                Expiry::Never | Expiry::Paused { .. } => None,
            })
            .min()
    }

    /// Stops `id`'s timer at `now`, keeping the time it has left. Pausing a
    /// paused notification, or one that never expires, changes nothing.
    /// `None` when it is not live.
    pub(crate) fn pause(&mut self, id: NotificationId, now: Instant) -> Option<()> {
        self.set_expiry(id, |expiry| expiry.paused(now))
    }

    /// Starts `id`'s timer again at `now` with the time it had left.
    /// `None` when it is not live.
    pub(crate) fn resume(&mut self, id: NotificationId, now: Instant) -> Option<()> {
        self.set_expiry(id, |expiry| expiry.resumed(now))
    }

    fn set_expiry(&mut self, id: NotificationId, f: impl FnOnce(Expiry) -> Expiry) -> Option<()> {
        let at = self.position(id)?;
        let expiry = f(self.live[at].expiry);
        if expiry != self.live[at].expiry {
            let mut notification = Notification::clone(&self.live[at]);
            notification.expiry = expiry;
            self.live[at] = Arc::new(notification);
        }
        Some(())
    }
}

/// Why an action could not be invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionRefused {
    NotFound,
    UnknownAction,
}

#[cfg(test)]
mod tests;
