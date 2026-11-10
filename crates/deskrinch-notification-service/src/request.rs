//! A `Notify` call as the sender made it, and the validated [`Notification`]
//! built from it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::capabilities::Capabilities;
use crate::image::{self, Image, ImageSource, RawImage};
use crate::limits::Limits;
use crate::markup;
use crate::notification::{Action, Expiry, Notification, NotificationId, Sound, Urgency};
use crate::policy::Ownership;
use crate::text::{self, Lines};

/// The arguments of a `Notify` call, unvalidated, borrowed from the message.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Request<'a> {
    pub(crate) sender: Option<String>,
    pub(crate) app_name: &'a str,
    pub(crate) replaces_id: u32,
    pub(crate) app_icon: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) body: &'a str,
    pub(crate) actions: Vec<&'a str>,
    pub(crate) hints: Hints<'a>,
    pub(crate) expire_timeout: i32,
}

/// The standard hints a `Notify` call carried, each already of its
/// specified type (a hint of the wrong type is left out when decoded).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Hints<'a> {
    pub(crate) urgency: Option<u8>,
    pub(crate) category: Option<&'a str>,
    pub(crate) desktop_entry: Option<&'a str>,
    /// `image-data`.
    pub(crate) image_data: Option<RawImage<'a>>,
    /// `image_data`, deprecated in 1.2.
    pub(crate) image_data_legacy: Option<RawImage<'a>>,
    /// `image-path`.
    pub(crate) image_path: Option<&'a str>,
    /// `image_path`, deprecated in 1.2.
    pub(crate) image_path_legacy: Option<&'a str>,
    /// `icon_data`, deprecated in 1.1.
    pub(crate) icon_data: Option<RawImage<'a>>,
    pub(crate) sound_file: Option<&'a str>,
    pub(crate) sound_name: Option<&'a str>,
    pub(crate) suppress_sound: Option<bool>,
    pub(crate) transient: Option<bool>,
    pub(crate) resident: Option<bool>,
    pub(crate) action_icons: Option<bool>,
    pub(crate) x: Option<i32>,
    pub(crate) y: Option<i32>,
}

/// How the model turns requests into notifications.
#[derive(Debug, Clone)]
pub(crate) struct Policy {
    pub(crate) limits: Limits,
    pub(crate) capabilities: Capabilities,
    /// The timeout of a notification that leaves it to the server (`-1`);
    /// `None` keeps those until dismissed.
    pub(crate) default_timeout: Option<Duration>,
    /// Which senders may replace or close a notification.
    pub(crate) ownership: Ownership,
}

impl Request<'_> {
    /// The notification this request makes, as `id`, shown at `now`.
    /// `image_budget` is how many bytes of decoded pixels it may hold.
    pub(crate) fn build(
        &self,
        id: NotificationId,
        now: Instant,
        policy: &Policy,
        image_budget: usize,
    ) -> Notification {
        let limits = &policy.limits;
        let caps = policy.capabilities;
        let hints = &self.hints;
        let urgency = hints
            .urgency
            .and_then(Urgency::from_level)
            .unwrap_or_default();
        let timeout = timeout(self.expire_timeout, urgency, policy.default_timeout);
        let mut budget = image_budget;
        let image = self.image(limits, &mut budget);
        let icon_data = hints
            .icon_data
            .and_then(|raw| decode(&raw, limits, &mut budget))
            .map(Arc::new);
        Notification {
            id,
            sender: self.sender.clone(),
            app_name: text::clean(self.app_name.trim(), limits.max_app_name_bytes, Lines::Join),
            app_icon: ImageSource::from_reference(self.app_icon, limits.max_reference_bytes),
            summary: text::clean(self.summary.trim(), limits.max_summary_bytes, Lines::Join),
            body: markup::parse(
                self.body,
                markup::Options {
                    hyperlinks: caps.body_hyperlinks,
                    max_markup_bytes: limits.max_body_markup_bytes,
                    max_text_bytes: limits.max_body_text_bytes,
                },
            ),
            actions: if caps.actions {
                self.actions(limits)
            } else {
                Vec::new()
            },
            action_icons: caps.action_icons_in_effect() && hints.action_icons == Some(true),
            urgency,
            category: hints
                .category
                .and_then(|c| text::non_empty(c, limits.max_hint_bytes)),
            desktop_entry: hints
                .desktop_entry
                .and_then(|d| text::non_empty(d, limits.max_hint_bytes)),
            image,
            icon_data,
            sound: Sound {
                file: hints
                    .sound_file
                    .filter(|f| f.len() <= limits.max_reference_bytes)
                    .and_then(image::file_reference),
                name: hints
                    .sound_name
                    .and_then(|n| text::non_empty(n, limits.max_hint_bytes)),
                suppress: hints.suppress_sound == Some(true),
            },
            transient: hints.transient == Some(true),
            resident: hints.resident == Some(true),
            position: hints.x.zip(hints.y),
            shown_at: now,
            timeout,
            expiry: Expiry::starting(timeout, now),
        }
    }

    /// The first valid image in the specification's order.
    fn image(&self, limits: &Limits, budget: &mut usize) -> Option<ImageSource> {
        let hints = &self.hints;
        let pixels = [hints.image_data, hints.image_data_legacy];
        let paths = [hints.image_path, hints.image_path_legacy];
        pixels
            .into_iter()
            .flatten()
            .find_map(|raw| decode(&raw, limits, budget))
            .map(|image| ImageSource::Pixels(Arc::new(image)))
            .or_else(|| {
                paths
                    .into_iter()
                    .flatten()
                    .find_map(|p| ImageSource::from_reference(p, limits.max_reference_bytes))
            })
    }

    /// The `(key, label)` pairs, bounded, keys unique and echoed back exactly
    /// as sent (so a key over its bound drops the action rather than being
    /// cut short).
    fn actions(&self, limits: &Limits) -> Vec<Action> {
        let mut actions: Vec<Action> = Vec::new();
        for &[key, label] in self.actions.as_chunks::<2>().0 {
            if actions.len() == limits.max_actions {
                break;
            }
            if key.is_empty()
                || key.len() > limits.max_action_key_bytes
                || actions.iter().any(|a| a.key == key)
            {
                continue;
            }
            actions.push(Action {
                key: key.to_owned(),
                label: text::clean(label.trim(), limits.max_action_label_bytes, Lines::Join),
            });
        }
        actions
    }
}

/// Decodes `raw` if it is valid and fits `budget`, taking its size from the
/// budget.
fn decode(raw: &RawImage<'_>, limits: &Limits, budget: &mut usize) -> Option<Image> {
    let image = Image::decode(raw, limits.max_image_side, limits.max_image_bytes).ok()?;
    *budget = budget.checked_sub(image.rgba().len())?;
    Some(image)
}

/// How long after it is shown a notification expires, if it does.
///
/// A critical notification never expires on its own, whatever its timeout
/// (the specification's urgency levels). Otherwise `0` is never, a positive
/// timeout is that many milliseconds, and `-1` (or any other negative value)
/// is the server's default.
fn timeout(expire_timeout: i32, urgency: Urgency, default: Option<Duration>) -> Option<Duration> {
    if urgency == Urgency::Critical {
        return None;
    }
    match u64::try_from(expire_timeout) {
        Ok(0) => None,
        Ok(ms) => Some(Duration::from_millis(ms)),
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests;
