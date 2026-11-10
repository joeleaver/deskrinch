//! A real-world client, libnotify's `notify-send`, against the service.

use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;

use deskrinch_notification_service::{CloseReason, Event, ImageSource, Urgency};

use crate::common::{PATIENCE, PrivateBus, start};

fn notify_send(bus: &PrivateBus, args: &[&str]) -> Child {
    Command::new("notify-send")
        .args(args)
        .env("DBUS_SESSION_BUS_ADDRESS", bus.address())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("notify-send (libnotify) must be installed to run these tests")
}

/// Waits for `child` to exit, within the tests' patience.
fn finish(child: Child) -> Output {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _sent = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(PATIENCE)
        .expect("notify-send did not exit in time")
        .unwrap();
    assert!(
        output.status.success(),
        "notify-send failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn notify_send_notifications_arrive_with_their_options() {
    let bus = PrivateBus::start();
    let (_service, events, _clock) = start(&bus);
    let child = notify_send(
        &bus,
        &[
            "--print-id",
            "--urgency=low",
            "--expire-time=3000",
            "--app-name=Interop",
            "--icon=dialog-information",
            "--category=im.received",
            "--transient",
            "--hint=int:x:40",
            "--hint=int:y:50",
            "--hint=string:desktop-entry:org.example.Interop",
            "Hello & welcome",
            "Some <b>bold</b> &amp; <i>italic</i>",
        ],
    );
    let output = finish(child);
    let n = events.next_shown();
    assert_eq!(stdout(&output).trim(), n.id.to_string());
    assert_eq!(n.app_name, "Interop");
    assert_eq!(n.summary, "Hello & welcome");
    assert_eq!(n.body.plain_text(), "Some bold & italic");
    assert!(n.body.spans()[1].style.bold && n.body.spans()[3].style.italic);
    assert_eq!(n.urgency, Urgency::Low);
    assert_eq!(n.timeout(), Some(std::time::Duration::from_secs(3)));
    assert_eq!(n.category.as_deref(), Some("im.received"));
    assert_eq!(n.desktop_entry.as_deref(), Some("org.example.Interop"));
    assert!(n.transient);
    assert_eq!(n.position, Some((40, 50)));
    match n.sole_image() {
        Some(ImageSource::Icon(name)) => assert_eq!(name.as_str(), "dialog-information"),
        other => panic!("expected the icon name, got {other:?}"),
    }
}

#[test]
fn notify_send_replaces_by_id() {
    let bus = PrivateBus::start();
    let (_service, events, _clock) = start(&bus);
    let output = finish(notify_send(&bus, &["--print-id", "First"]));
    let id = stdout(&output).trim().to_owned();
    events.next_shown();
    let output = finish(notify_send(
        &bus,
        &["--print-id", &format!("--replace-id={id}"), "Second"],
    ));
    assert_eq!(stdout(&output).trim(), id);
    match events.next() {
        Event::Replaced(n) => {
            assert_eq!(n.id.to_string(), id);
            assert_eq!(n.summary, "Second");
        }
        other => panic!("expected Replaced, got {other:?}"),
    }
}

#[test]
fn notify_send_hears_the_invoked_action_and_its_token() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let child = notify_send(
        &bus,
        &[
            "--action=reply=Reply",
            "--action=later=Later",
            "--activation-token-fd=1",
            "Question",
        ],
    );
    let n = events.next_shown();
    let keys: Vec<_> = n.actions.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(keys, ["reply", "later"]);
    service
        .handle()
        .invoke_action(n.id, "reply", Some("token-from-the-ui".into()))
        .unwrap();
    let output = finish(child);
    let printed = stdout(&output);
    let lines: Vec<_> = printed.lines().collect();
    assert!(lines.contains(&"reply"), "notify-send printed {printed:?}");
    assert!(
        printed.contains("token-from-the-ui"),
        "notify-send printed {printed:?}"
    );
    assert_eq!(
        events.next(),
        Event::Closed {
            id: n.id,
            reason: CloseReason::Dismissed
        }
    );
}

#[test]
fn notify_send_waits_until_dismissed() {
    let bus = PrivateBus::start();
    let (service, events, _clock) = start(&bus);
    let child = notify_send(&bus, &["--wait", "Waiting"]);
    let n = events.next_shown();
    service.handle().dismiss(n.id).unwrap();
    finish(child);
}
