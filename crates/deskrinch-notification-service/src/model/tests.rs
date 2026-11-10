use std::time::Duration;

use super::*;
use crate::capabilities::Capabilities;
use crate::limits::Limits;
use crate::notification::Urgency;
use crate::request::Hints;

const SECOND: Duration = Duration::from_secs(1);

fn model() -> Model {
    model_with(Limits::default())
}

fn model_with(limits: Limits) -> Model {
    Model::new(Policy {
        limits,
        capabilities: Capabilities::default(),
        default_timeout: Some(5 * SECOND),
    })
}

fn request(summary: &str) -> Request<'_> {
    Request {
        sender: Some(":1.1".into()),
        summary,
        expire_timeout: -1,
        ..Request::default()
    }
}

fn id(n: u32) -> NotificationId {
    NotificationId::new(n).unwrap()
}

fn summaries(model: &Model) -> Vec<&str> {
    model.live().iter().map(|n| n.summary.as_str()).collect()
}

#[test]
fn notifications_are_shown_in_order_with_increasing_ids() {
    let mut m = model();
    let now = Instant::now();
    let (a, change) = m.notify(&request("a"), now).unwrap();
    assert!(matches!(change, Change::Shown(ref n) if n.id == a && n.summary == "a"));
    let (b, _) = m.notify(&request("b"), now).unwrap();
    assert_eq!((a.get(), b.get()), (1, 2));
    assert_eq!(summaries(&m), ["a", "b"]);
}

#[test]
fn replacement_keeps_the_id_and_the_place_and_restarts_the_timer() {
    let mut m = model();
    let t0 = Instant::now();
    let (a, _) = m.notify(&request("a"), t0).unwrap();
    m.notify(&request("b"), t0).unwrap();
    let replacement = Request {
        replaces_id: a.get(),
        ..request("a2")
    };
    let (same, change) = m.notify(&replacement, t0 + 3 * SECOND).unwrap();
    assert_eq!(same, a);
    assert!(matches!(change, Change::Replaced(ref n) if n.id == a && n.summary == "a2"));
    assert_eq!(summaries(&m), ["a2", "b"]);
    assert_eq!(m.next_expiry(), Some(t0 + 5 * SECOND), "b expires first");
    assert_eq!(m.expire(t0 + 5 * SECOND).len(), 1);
    assert_eq!(m.next_expiry(), Some(t0 + 8 * SECOND));
}

#[test]
fn replacing_an_id_that_is_not_live_shows_a_new_notification() {
    let mut m = model();
    let now = Instant::now();
    let (id, change) = m
        .notify(
            &Request {
                replaces_id: 42,
                ..request("a")
            },
            now,
        )
        .unwrap();
    assert_eq!(id.get(), 1);
    assert!(matches!(change, Change::Shown(_)));
}

#[test]
fn expiry_closes_due_notifications_only() {
    let mut m = model();
    let t0 = Instant::now();
    let explicit = Request {
        expire_timeout: 1000,
        ..request("explicit")
    };
    let never = Request {
        expire_timeout: 0,
        ..request("never")
    };
    let critical = Request {
        hints: Hints {
            urgency: Some(2),
            ..Hints::default()
        },
        expire_timeout: 1000,
        ..request("critical")
    };
    let (e, _) = m.notify(&explicit, t0).unwrap();
    m.notify(&request("default"), t0).unwrap();
    m.notify(&never, t0).unwrap();
    m.notify(&critical, t0).unwrap();
    assert_eq!(m.live()[3].urgency, Urgency::Critical);

    assert_eq!(m.expire(t0 + Duration::from_millis(999)), []);
    assert_eq!(
        m.expire(t0 + SECOND),
        [Change::Closed {
            id: e,
            reason: CloseReason::Expired
        }]
    );
    assert_eq!(m.expire(t0 + 5 * SECOND).len(), 1);
    assert_eq!(summaries(&m), ["never", "critical"]);
    assert_eq!(m.next_expiry(), None);
    assert_eq!(m.expire(t0 + 1_000_000 * SECOND), []);
}

#[test]
fn close_and_dismiss_report_their_reason() {
    let mut m = model();
    let now = Instant::now();
    let (a, _) = m.notify(&request("a"), now).unwrap();
    let (b, _) = m.notify(&request("b"), now).unwrap();
    assert_eq!(
        m.close(a, CloseReason::Closed),
        Some(Change::Closed {
            id: a,
            reason: CloseReason::Closed
        })
    );
    assert_eq!(m.close(a, CloseReason::Closed), None, "already gone");
    assert!(matches!(
        m.close(b, CloseReason::Dismissed),
        Some(Change::Closed {
            reason: CloseReason::Dismissed,
            ..
        })
    ));
    assert_eq!(m.live(), []);
}

#[test]
fn invoking_an_action_dismisses_unless_resident() {
    let mut m = model();
    let now = Instant::now();
    let actions = vec!["default", "Open", "reply", "Reply"];
    let plain = Request {
        actions: actions.clone(),
        ..request("plain")
    };
    let resident = Request {
        actions,
        hints: Hints {
            resident: Some(true),
            ..Hints::default()
        },
        ..request("resident")
    };
    let (p, _) = m.notify(&plain, now).unwrap();
    let (r, _) = m.notify(&resident, now).unwrap();

    let changes = m.invoke_action(p, "reply", Some("token".into())).unwrap();
    assert_eq!(
        changes,
        [
            Change::ActionInvoked {
                id: p,
                key: "reply".into(),
                activation_token: Some("token".into())
            },
            Change::Closed {
                id: p,
                reason: CloseReason::Dismissed
            }
        ]
    );
    let changes = m.invoke_action(r, "default", None).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(summaries(&m), ["resident"]);

    assert_eq!(
        m.invoke_action(r, "forward", None),
        Err(ActionRefused::UnknownAction)
    );
    assert_eq!(
        m.invoke_action(p, "reply", None),
        Err(ActionRefused::NotFound)
    );
}

#[test]
fn live_notifications_are_bounded_in_total_and_per_sender() {
    let mut m = model_with(Limits {
        max_live: 3,
        max_live_per_sender: 2,
        ..Limits::default()
    });
    let now = Instant::now();
    let from = |sender: &str| Request {
        sender: Some(sender.into()),
        ..request("x")
    };
    let (first, _) = m.notify(&from(":1.1"), now).unwrap();
    m.notify(&from(":1.1"), now).unwrap();
    assert_eq!(
        m.notify(&from(":1.1"), now),
        Err(Refused::TooManyFromSender(2))
    );
    // Replacing its own notification costs a sender nothing.
    let own = Request {
        replaces_id: first.get(),
        ..from(":1.1")
    };
    assert!(m.notify(&own, now).is_ok());
    m.notify(&from(":1.2"), now).unwrap();
    assert_eq!(m.notify(&from(":1.3"), now), Err(Refused::TooMany(3)));
    // A sender at its bound cannot take over another's notification either.
    m.close(first, CloseReason::Closed);
    let (other, _) = m.notify(&from(":1.3"), now).unwrap();
    m.notify(&from(":1.2"), now).unwrap_err();
    let takeover = Request {
        replaces_id: other.get(),
        ..from(":1.1")
    };
    assert!(
        m.notify(&takeover, now).is_ok(),
        ":1.1 has one live notification"
    );
}

#[test]
fn ids_wrap_skipping_zero_and_live_ids() {
    let mut m = model();
    let now = Instant::now();
    let (one, _) = m.notify(&request("one"), now).unwrap();
    m.next_id = u32::MAX;
    let (max, _) = m.notify(&request("max"), now).unwrap();
    assert_eq!(max.get(), u32::MAX);
    let (next, _) = m.notify(&request("next"), now).unwrap();
    assert_eq!(one.get(), 1);
    assert_eq!(next.get(), 2, "0 is never an id and 1 is still live");
    assert_eq!(id(2), next);
}

#[test]
fn the_image_budget_is_shared_by_live_notifications() {
    const PIXEL: [u8; 4] = [0; 4];
    let image = crate::image::RawImage {
        width: 1,
        height: 1,
        rowstride: 4,
        has_alpha: true,
        bits_per_sample: 8,
        channels: 4,
        data: &PIXEL,
    };
    let mut m = model_with(Limits {
        max_image_bytes_total: 4,
        ..Limits::default()
    });
    let now = Instant::now();
    let with_image = Request {
        hints: Hints {
            image_data: Some(image),
            ..Hints::default()
        },
        ..request("x")
    };
    let (a, change) = m.notify(&with_image, now).unwrap();
    assert!(matches!(change, Change::Shown(ref n) if n.image.is_some()));
    let (_, change) = m.notify(&with_image, now).unwrap();
    assert!(matches!(change, Change::Shown(ref n) if n.image.is_none()));
    // Replacing a notification frees its own share first.
    let replacement = Request {
        replaces_id: a.get(),
        ..with_image
    };
    let (_, change) = m.notify(&replacement, now).unwrap();
    assert!(matches!(change, Change::Replaced(ref n) if n.image.is_some()));
}
