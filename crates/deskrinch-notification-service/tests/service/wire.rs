//! What a client sends arrives validated and parsed; what the service
//! advertises is what it supports.

use std::collections::HashMap;

use deskrinch_notification_service::{Config, ImageSource, Limits, Urgency};
use zbus::Variant;

use crate::common::{Client, PrivateBus, start, start_with};

#[test]
fn server_information_and_capabilities() {
    let bus = PrivateBus::start();
    let (_service, _events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let (name, vendor, version, spec) =
        zbus::block_on(client.proxy.get_server_information()).unwrap();
    assert_eq!((name.as_str(), vendor.as_str()), ("deskrinch", "deskrinch"));
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(spec, "1.3");
    let caps = zbus::block_on(client.proxy.get_capabilities()).unwrap();
    assert_eq!(
        caps,
        [
            "body",
            "body-markup",
            "icon-static",
            "actions",
            "action-icons",
            "body-hyperlinks"
        ]
    );
}

#[test]
fn a_notification_arrives_parsed_with_its_hints() {
    let bus = PrivateBus::start();
    let (_service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    let image = (
        2i32,
        1i32,
        6i32,
        false,
        8i32,
        3i32,
        vec![1u8, 2, 3, 4, 5, 6],
    );
    let hints = HashMap::from([
        ("urgency", Variant::from(2u8)),
        ("category", Variant::from("email.arrived")),
        ("desktop-entry", Variant::from("org.example.Mail")),
        ("image-data", Variant::from(image)),
        (
            "image-path",
            Variant::from("/ignored/because/image-data/wins.png"),
        ),
        ("x-vendor-hint", Variant::from(7u64)),
    ]);
    let actions = ["default", "Open", "archive", "Archive"];
    let id = client
        .send(
            0,
            "New mail\u{1b}",
            r#"<b>From</b> &lt;a@b.c&gt; <a href="https://e.org">link</a> <a href="javascript:x">js</a><script>"#,
            &actions,
            &hints,
            -1,
        )
        .unwrap();
    let n = events.next_shown();
    assert_eq!(n.id.get(), id);
    assert!(n.sender.as_deref().is_some_and(|s| s.starts_with(':')));
    assert_eq!(n.app_name, "Test");
    assert_eq!(n.summary, "New mail");
    assert_eq!(n.body.plain_text(), "From <a@b.c> link js");
    let spans = n.body.spans();
    assert!(spans[0].style.bold);
    assert_eq!(
        spans[2]
            .link
            .as_ref()
            .map(deskrinch_notification_service::Link::as_str),
        Some("https://e.org")
    );
    assert_eq!(spans[3].link, None);
    assert_eq!(n.urgency, Urgency::Critical);
    assert_eq!(n.timeout(), None, "critical never expires");
    assert_eq!(n.category.as_deref(), Some("email.arrived"));
    assert_eq!(n.desktop_entry.as_deref(), Some("org.example.Mail"));
    let Some(ImageSource::Pixels(image)) = &n.image else {
        panic!("expected decoded pixels, got {:?}", n.image);
    };
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(image.rgba(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    let keys: Vec<_> = n.actions.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(keys, ["default", "archive"]);
}

#[test]
fn an_invalid_image_falls_back_to_the_image_path() {
    let bus = PrivateBus::start();
    let (_service, events, _clock) = start(&bus);
    let client = Client::new(&bus);
    // Claims 100×100 RGBA but carries four bytes.
    let image = (100i32, 100i32, 400i32, true, 8i32, 4i32, vec![0u8; 4]);
    let hints = HashMap::from([
        ("image-data", Variant::from(image)),
        (
            "image-path",
            Variant::from("file:///tmp/fallback%20image.png"),
        ),
    ]);
    client.send(0, "s", "", &[], &hints, -1).unwrap();
    let n = events.next_shown();
    assert_eq!(
        n.image,
        Some(ImageSource::File("/tmp/fallback image.png".into()))
    );
}

#[test]
fn a_sender_over_its_bound_gets_an_error() {
    let bus = PrivateBus::start();
    let config = Config {
        limits: Limits {
            max_live_per_sender: 2,
            ..Limits::default()
        },
        ..bus.config()
    };
    let clock = std::sync::Arc::new(deskrinch_notification_service::ManualClock::new());
    let (_service, events) = start_with(config, clock);
    let client = Client::new(&bus);
    client.simple(0, "1", "", 0);
    client.simple(0, "2", "", 0);
    let err = client
        .send(0, "3", "", &[], &HashMap::new(), 0)
        .unwrap_err();
    let zbus::Error::MethodError(name, _, _) = &err else {
        panic!("expected a method error, got {err:?}");
    };
    assert_eq!(
        name.as_str(),
        "org.freedesktop.Notifications.Error.LimitsExceeded"
    );
    // Another sender is not held to the first one's bound.
    Client::new(&bus).simple(0, "other", "", 0);
    let summaries: Vec<_> = (0..3)
        .map(|_| events.next_shown().summary.clone())
        .collect();
    assert_eq!(summaries, ["1", "2", "other"]);
}
