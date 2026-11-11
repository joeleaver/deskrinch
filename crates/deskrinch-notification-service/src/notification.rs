//! A live notification as the UI sees it.

use std::fmt;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::image::{Image, ImageSource};
use deskrinch_untrusted::markup::Body;

/// A notification's id: unique among live notifications, never 0, and not
/// reused until the 32-bit counter wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NotificationId(NonZeroU32);

impl NotificationId {
    /// The id `value`, or `None` for 0, which is never an id.
    #[must_use]
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    /// The id as it goes over D-Bus.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for NotificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// How urgent a notification is (the `urgency` hint).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Urgency {
    /// `0`.
    Low,
    /// `1`, and what a notification without the hint has.
    #[default]
    Normal,
    /// `2`. A critical notification never expires on its own.
    Critical,
}

impl Urgency {
    /// The urgency the hint's byte names; `None` for an unknown level.
    pub(crate) fn from_level(level: u8) -> Option<Self> {
        match level {
            0 => Some(Self::Low),
            1 => Some(Self::Normal),
            2 => Some(Self::Critical),
            _ => None,
        }
    }
}

/// An action the user can invoke, from the sender's `actions` list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Action {
    /// The key the sender gets back in `ActionInvoked`. With the
    /// `action-icons` hint it is also an icon name.
    pub key: String,
    /// The text to show for it, single line.
    pub label: String,
}

impl Action {
    /// The key of the action invoked by activating the notification itself,
    /// rather than one of its buttons.
    pub const DEFAULT_KEY: &'static str = "default";
}

/// The sound hints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sound {
    /// `sound-file`: an absolute path to play.
    pub file: Option<PathBuf>,
    /// `sound-name`: a freedesktop.org sound theme name to play.
    pub name: Option<String>,
    /// `suppress-sound`: play nothing, the sender does its own.
    pub suppress: bool,
}

/// When a notification expires on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expiry {
    /// Never: it stays until the user dismisses it or the sender closes it.
    Never,
    /// At this moment on the service's [`Clock`](crate::Clock).
    At(Instant),
    /// Paused by the UI ([`Handle::pause`](crate::Handle::pause)), with this
    /// much of its timeout left for when it resumes.
    Paused {
        /// The time left.
        remaining: Duration,
    },
}

impl Expiry {
    /// The expiry of a timeout starting at `now`.
    pub(crate) fn starting(timeout: Option<Duration>, now: Instant) -> Self {
        timeout.map_or(Self::Never, |after| Self::At(now + after))
    }

    /// This expiry paused at `now`; one that never comes is unchanged.
    #[must_use]
    pub(crate) fn paused(self, now: Instant) -> Self {
        match self {
            Self::At(at) => Self::Paused {
                remaining: at.saturating_duration_since(now),
            },
            other => other,
        }
    }

    /// This expiry running again from `now`.
    #[must_use]
    pub(crate) fn resumed(self, now: Instant) -> Self {
        match self {
            Self::Paused { remaining } => Self::At(now + remaining),
            other => other,
        }
    }
}

/// Why a notification left the model, as `NotificationClosed` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseReason {
    /// Its timeout passed (reason 1).
    Expired,
    /// The user dismissed it, or invoked an action on a notification that is
    /// not resident (reason 2).
    Dismissed,
    /// The sender called `CloseNotification` (reason 3).
    Closed,
    /// The service stopped, or lost `org.freedesktop.Notifications` to
    /// another service (reason 4, "Undefined/reserved reasons" in the
    /// specification). Senders waiting on the notification are released.
    Undefined,
}

impl CloseReason {
    /// The reason code `NotificationClosed` carries.
    #[must_use]
    pub fn code(self) -> u32 {
        match self {
            Self::Expired => 1,
            Self::Dismissed => 2,
            Self::Closed => 3,
            Self::Undefined => 4,
        }
    }
}

/// A live notification, everything in it validated and bounded.
///
/// Every string is clean of control characters (the body keeps line breaks
/// and tabs) and within its [`Limits`](crate::Limits). Nothing here needs
/// further interpretation: the body is already parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Notification {
    /// Its id.
    pub id: NotificationId,
    /// The unique bus name of the connection that sent (or last replaced) it.
    pub sender: Option<String>,
    /// The sending application's name, single line; may be empty.
    pub app_name: String,
    /// The `app_icon` argument.
    pub app_icon: Option<ImageSource>,
    /// The summary, single line, plain text.
    pub summary: String,
    /// The parsed body.
    pub body: Body,
    /// The actions, in the sender's order, keys unique.
    pub actions: Vec<Action>,
    /// The `action-icons` hint: show each action as the icon its key names.
    pub action_icons: bool,
    /// The `urgency` hint.
    pub urgency: Urgency,
    /// The `category` hint, such as `email.arrived`.
    pub category: Option<String>,
    /// The `desktop-entry` hint: the sender's desktop file name, without
    /// `.desktop`.
    pub desktop_entry: Option<String>,
    /// The image from `image-data`, `image-path` or their deprecated
    /// spellings, in the specification's order, the first that is valid.
    pub image: Option<ImageSource>,
    /// The deprecated `icon_data` hint, the last resort of both orders.
    pub icon_data: Option<Arc<Image>>,
    /// The sound hints.
    pub sound: Sound,
    /// The `transient` hint: not to be kept in a history.
    pub transient: bool,
    /// The `resident` hint: invoking an action does not close it.
    pub resident: bool,
    /// The `x` and `y` hints: the screen point it should point to.
    pub position: Option<(i32, i32)>,
    /// When it was shown or last replaced, on the service's clock.
    pub shown_at: Instant,
    /// How long after it was shown it expires, if it does: the sender's
    /// timeout, or the default, or `None` (also for critical urgency).
    pub timeout: Option<Duration>,
    /// When it expires.
    pub expiry: Expiry,
}

impl Notification {
    /// The image for a UI that shows an image beside the application icon:
    /// [`image`](Self::image), else [`icon_data`](Self::icon_data).
    #[must_use]
    pub fn image_with_icon(&self) -> Option<ImageSource> {
        self.image
            .clone()
            .or_else(|| self.icon_data.clone().map(ImageSource::Pixels))
    }

    /// The one picture for a UI that shows only one, in the specification's
    /// order: `image-data`, `image-path`, `app_icon`, `icon_data`.
    #[must_use]
    pub fn sole_image(&self) -> Option<ImageSource> {
        self.image
            .clone()
            .or_else(|| self.app_icon.clone())
            .or_else(|| self.icon_data.clone().map(ImageSource::Pixels))
    }

    /// The action invoked by activating the notification itself, if the
    /// sender offered one.
    #[must_use]
    pub fn default_action(&self) -> Option<&Action> {
        self.action(Action::DEFAULT_KEY)
    }

    /// The action with `key`.
    #[must_use]
    pub fn action(&self, key: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.key == key)
    }

    /// Bytes of decoded pixels this notification holds.
    pub(crate) fn image_bytes(&self) -> usize {
        let pixels = |s: &Option<ImageSource>| match s {
            Some(ImageSource::Pixels(image)) => image.rgba().len(),
            _ => 0,
        };
        pixels(&self.image)
            + pixels(&self.app_icon)
            + self.icon_data.as_ref().map_or(0, |i| i.rgba().len())
    }
}
