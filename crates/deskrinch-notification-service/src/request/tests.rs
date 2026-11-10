use std::path::PathBuf;

use super::*;
use crate::image::IconName;

fn policy() -> Policy {
    Policy {
        limits: Limits::default(),
        capabilities: Capabilities::default(),
        default_timeout: Some(Duration::from_secs(5)),
        ownership: Ownership::AnySender,
    }
}

fn build(request: &Request<'_>) -> Notification {
    build_with(request, &policy())
}

fn build_with(request: &Request<'_>, policy: &Policy) -> Notification {
    let id = NotificationId::new(7).unwrap();
    request.build(id, Instant::now(), policy, usize::MAX)
}

const RGBA_1X1: [u8; 4] = [1, 2, 3, 4];

fn pixel() -> RawImage<'static> {
    RawImage {
        width: 1,
        height: 1,
        rowstride: 4,
        has_alpha: true,
        bits_per_sample: 8,
        channels: 4,
        data: &RGBA_1X1,
    }
}

fn broken() -> RawImage<'static> {
    RawImage {
        channels: 3,
        ..pixel()
    }
}

#[test]
fn strings_are_cleaned_and_bounded() {
    let mut p = policy();
    p.limits.max_summary_bytes = 5;
    let n = build_with(
        &Request {
            app_name: " Mail\n",
            summary: "Hello\u{1b} world",
            body: "<b>hi</b>",
            ..Request::default()
        },
        &p,
    );
    assert_eq!(n.app_name, "Mail");
    assert_eq!(n.summary, "Hello");
    assert_eq!(n.body.plain_text(), "hi");
    assert!(n.body.spans()[0].style.bold);
}

#[test]
fn actions_pair_up_with_unique_bounded_keys() {
    let mut p = policy();
    p.limits.max_actions = 3;
    p.limits.max_action_key_bytes = 8;
    let actions = vec![
        "default",
        "Open",
        "",
        "Empty key",
        "reply",
        "Reply\nnow",
        "reply",
        "Duplicate",
        "much-too-long",
        "Long",
        "archive",
        "Archive",
        "later",
        "Over the bound",
        "odd",
    ];
    let n = build_with(
        &Request {
            actions,
            ..Request::default()
        },
        &p,
    );
    let got: Vec<_> = n
        .actions
        .iter()
        .map(|a| (a.key.as_str(), a.label.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("default", "Open"),
            ("reply", "Reply now"),
            ("archive", "Archive")
        ]
    );
    assert_eq!(n.default_action().map(|a| a.label.as_str()), Some("Open"));
}

#[test]
fn actions_are_dropped_without_the_capability() {
    let mut p = policy();
    p.capabilities.actions = false;
    let n = build_with(
        &Request {
            actions: vec!["default", "Open"],
            hints: Hints {
                action_icons: Some(true),
                ..Hints::default()
            },
            ..Request::default()
        },
        &p,
    );
    assert_eq!(n.actions, []);
    assert!(!n.action_icons);
}

#[test]
fn image_data_wins_over_image_path_and_app_icon() {
    let request = Request {
        app_icon: "mail-client",
        hints: Hints {
            image_data: Some(pixel()),
            image_path: Some("/tmp/a.png"),
            icon_data: Some(pixel()),
            ..Hints::default()
        },
        ..Request::default()
    };
    let n = build(&request);
    assert!(matches!(n.image, Some(ImageSource::Pixels(_))));
    assert!(matches!(n.sole_image(), Some(ImageSource::Pixels(_))));
    assert_eq!(
        n.app_icon,
        Some(ImageSource::Icon(IconName::parse("mail-client").unwrap()))
    );
}

#[test]
fn invalid_image_data_falls_back_in_order() {
    let request = Request {
        app_icon: "mail-client",
        hints: Hints {
            image_data: Some(broken()),
            image_data_legacy: Some(broken()),
            image_path: Some("not a valid reference"),
            image_path_legacy: Some("file:///tmp/legacy.png"),
            ..Hints::default()
        },
        ..Request::default()
    };
    let n = build(&request);
    assert_eq!(
        n.image,
        Some(ImageSource::File(PathBuf::from("/tmp/legacy.png")))
    );
}

#[test]
fn app_icon_comes_before_icon_data_when_only_one_is_shown() {
    let request = Request {
        app_icon: "mail-client",
        hints: Hints {
            icon_data: Some(pixel()),
            ..Hints::default()
        },
        ..Request::default()
    };
    let n = build(&request);
    assert!(matches!(n.sole_image(), Some(ImageSource::Icon(_))));
    assert!(matches!(n.image_with_icon(), Some(ImageSource::Pixels(_))));
    let without_icon = build(&Request {
        app_icon: "",
        ..request
    });
    assert!(matches!(
        without_icon.sole_image(),
        Some(ImageSource::Pixels(_))
    ));
}

#[test]
fn images_over_the_budget_are_dropped() {
    let request = Request {
        hints: Hints {
            image_data: Some(pixel()),
            image_path: Some("/tmp/a.png"),
            icon_data: Some(pixel()),
            ..Hints::default()
        },
        ..Request::default()
    };
    let id = NotificationId::new(1).unwrap();
    let n = request.build(id, Instant::now(), &policy(), 4);
    assert!(matches!(n.image, Some(ImageSource::Pixels(_))));
    assert_eq!(n.icon_data, None, "the budget was spent on image-data");
    let n = request.build(id, Instant::now(), &policy(), 3);
    assert_eq!(
        n.image,
        Some(ImageSource::File(PathBuf::from("/tmp/a.png")))
    );
    assert_eq!(n.image_bytes(), 0);
}

#[test]
fn hints_are_carried_over() {
    let request = Request {
        hints: Hints {
            urgency: Some(0),
            category: Some(" email.arrived "),
            desktop_entry: Some("org.example.Mail"),
            sound_file: Some("/usr/share/sounds/a.oga"),
            sound_name: Some("message-new-email"),
            suppress_sound: Some(true),
            transient: Some(true),
            resident: Some(true),
            action_icons: Some(true),
            x: Some(10),
            y: Some(20),
            ..Hints::default()
        },
        ..Request::default()
    };
    let n = build(&request);
    assert_eq!(n.urgency, Urgency::Low);
    assert_eq!(n.category.as_deref(), Some("email.arrived"));
    assert_eq!(n.desktop_entry.as_deref(), Some("org.example.Mail"));
    assert_eq!(n.sound.file, Some(PathBuf::from("/usr/share/sounds/a.oga")));
    assert_eq!(n.sound.name.as_deref(), Some("message-new-email"));
    assert!(n.sound.suppress && n.transient && n.resident && n.action_icons);
    assert_eq!(n.position, Some((10, 20)));
}

#[test]
fn position_needs_both_coordinates_and_urgency_must_be_known() {
    let request = Request {
        hints: Hints {
            urgency: Some(9),
            x: Some(10),
            sound_file: Some("relative.oga"),
            ..Hints::default()
        },
        ..Request::default()
    };
    let n = build(&request);
    assert_eq!(n.position, None);
    assert_eq!(n.urgency, Urgency::Normal);
    assert_eq!(n.sound.file, None);
}

#[test]
fn the_timeout_follows_the_request_and_urgency() {
    let default = Some(Duration::from_secs(5));
    let ms = |ms| Some(Duration::from_millis(ms));
    assert_eq!(timeout(-1, Urgency::Normal, default), ms(5000));
    assert_eq!(timeout(-7, Urgency::Normal, default), ms(5000));
    assert_eq!(timeout(-1, Urgency::Normal, None), None);
    assert_eq!(timeout(0, Urgency::Normal, default), None);
    assert_eq!(timeout(1500, Urgency::Low, default), ms(1500));
    assert_eq!(timeout(1500, Urgency::Critical, default), None);
    assert_eq!(timeout(-1, Urgency::Critical, default), None);
}

#[test]
fn the_expiry_starts_when_shown_and_pauses_keeping_the_rest() {
    let now = Instant::now();
    let second = Duration::from_secs(1);
    let expiry = Expiry::starting(Some(5 * second), now);
    assert_eq!(expiry, Expiry::At(now + 5 * second));
    let paused = expiry.paused(now + 2 * second);
    assert_eq!(
        paused,
        Expiry::Paused {
            remaining: 3 * second
        }
    );
    assert_eq!(paused.paused(now + 4 * second), paused, "pausing twice");
    assert_eq!(
        paused.resumed(now + 10 * second),
        Expiry::At(now + 13 * second)
    );
    assert_eq!(Expiry::starting(None, now).paused(now), Expiry::Never);
    // Paused past its deadline, it has nothing left, and expires on resume.
    assert_eq!(
        expiry.paused(now + 9 * second),
        Expiry::Paused {
            remaining: Duration::ZERO
        }
    );
}
