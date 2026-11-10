//! The daemon's choices about which senders may touch a notification and
//! who hears about it.

/// Which senders may replace or close a live notification.
///
/// A sender is a D-Bus connection (its unique name).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ownership {
    /// Any sender, as the specification leaves it. `notify-send -r` needs
    /// this: each run is a new connection. A replacement makes the replacing
    /// sender the notification's current sender.
    #[default]
    AnySender,
    /// Only the sender that showed it. Another sender's `Notify` naming it,
    /// or `CloseNotification` on it, gets an `AccessDenied` error.
    OriginalSender,
}

/// Who receives `NotificationClosed`, `ActionInvoked` and `ActivationToken`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SignalDelivery {
    /// Only the notification's current sender. An activation token lets its
    /// holder take focus, so it should go to nobody else; the other two
    /// signals tell other applications nothing they need.
    #[default]
    Sender,
    /// Every connection on the bus that listens, as most servers do.
    Broadcast,
}
