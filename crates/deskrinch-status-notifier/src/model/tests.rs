use std::time::Duration;

use deskrinch_untrusted::Image;

use super::*;
use crate::item::Icon;
use crate::parse;
use crate::wire::Properties;

const PATH: &str = "/StatusNotifierItem";

fn target(service: &str) -> Target {
    Target {
        service: service.into(),
        path: PATH.into(),
    }
}

fn model() -> Model {
    Model::new(Limits::default())
}

/// A fixed point in time, and times after it.
fn t(ms: u64) -> Instant {
    use std::sync::OnceLock;
    static ZERO: OnceLock<Instant> = OnceLock::new();
    *ZERO.get_or_init(Instant::now) + Duration::from_millis(ms)
}

/// The item `id` titled `title`.
fn item(id: &ItemId, title: &str) -> Item {
    let properties = Properties {
        title: Some(title.into()),
        ..Properties::default()
    };
    parse::item(id.clone(), properties, &Limits::default())
}

fn read_of(id: &ItemId, title: &str) -> Outcome {
    Outcome::Read(item(id, title).into())
}

/// Registers `service`, owned by itself, at `t(0)`; the id.
fn register(model: &mut Model, service: &str) -> ItemId {
    model
        .register(target(service), service, t(0))
        .unwrap()
        .unwrap()
        .id
}

/// Registers `service` and reads it as titled `title`; the id.
fn read(model: &mut Model, service: &str, title: &str) -> ItemId {
    let id = register(model, service);
    let done = model.read_done(&id, service, read_of(&id, title), t(0));
    assert!(matches!(done.change, Some(Change::Added(_))));
    id
}

#[test]
fn a_registered_item_is_listed_but_pending_until_it_is_read() {
    let mut model = model();
    let due = model
        .register(target(":1.5"), ":1.5", t(0))
        .unwrap()
        .unwrap();
    let id = due.id.clone();
    assert_eq!(id.as_str(), ":1.5/StatusNotifierItem");
    assert_eq!(
        due,
        Due {
            id: id.clone(),
            at: t(0)
        }
    );
    assert_eq!(
        model.source(&id),
        Some(Source {
            id: id.clone(),
            owner: ":1.5".into(),
            path: PATH.into()
        })
    );
    assert_eq!(model.registered(), [":1.5/StatusNotifierItem"]);
    assert_eq!(model.items(), Vec::<Arc<Item>>::new());
    assert_eq!(model.target(&id), None);

    let done = model.read_done(&id, ":1.5", read_of(&id, "A"), t(0));
    assert_eq!(done.change, Some(Change::Added(model.item(&id).unwrap())));
    assert_eq!(model.items().len(), 1);
    assert_eq!(model.target(&id), Some(target(":1.5")));
}

#[test]
fn an_item_registered_by_a_well_known_name_is_read_from_its_owner() {
    let mut model = model();
    let service = "org.kde.StatusNotifierItem-1-1";
    let due = model
        .register(target(service), ":1.5", t(0))
        .unwrap()
        .unwrap();
    assert_eq!(model.source(&due.id).unwrap().owner, ":1.5");
    // Its calls without a reply still go to the name it registered.
    model.read_done(&due.id, ":1.5", read_of(&due.id, "A"), t(0));
    assert_eq!(model.target(&due.id), Some(target(service)));
}

#[test]
fn registering_again_changes_nothing() {
    let mut model = model();
    register(&mut model, ":1.5");
    assert_eq!(model.register(target(":1.5"), ":1.5", t(0)), Ok(None));
    assert_eq!(model.registered().len(), 1);
}

#[test]
fn registrations_are_bounded_in_all_and_per_connection() {
    let mut model = Model::new(Limits {
        max_items: 3,
        max_items_per_connection: 2,
        ..Limits::default()
    });
    let path = |p: &str| Target {
        service: ":1.1".into(),
        path: p.into(),
    };
    model.register(path("/a"), ":1.1", t(0)).unwrap();
    model.register(path("/b"), ":1.1", t(0)).unwrap();
    assert_eq!(
        model.register(path("/c"), ":1.1", t(0)),
        Err(Refusal::PerConnection)
    );
    register(&mut model, ":1.2");
    assert_eq!(
        model.register(target(":1.3"), ":1.3", t(0)),
        Err(Refusal::Items)
    );
}

#[test]
fn a_failed_read_leaves_a_pending_item_pending_and_a_read_one_as_it_was() {
    let mut model = model();
    let pending = register(&mut model, ":1.5");
    assert_eq!(
        model.read_done(&pending, ":1.5", Outcome::Failed, t(0)),
        ReadDone::default()
    );
    assert_eq!(model.items(), Vec::<Arc<Item>>::new());

    let id = read(&mut model, ":1.6", "A");
    assert_eq!(model.signalled(":1.6", PATH, t(500)).len(), 1);
    assert_eq!(
        model.read_done(&id, ":1.6", Outcome::Failed, t(500)),
        ReadDone::default()
    );
    assert_eq!(model.item(&id).unwrap().title, "A");
}

#[test]
fn a_later_read_reports_a_change_only_when_something_changed() {
    let mut model = model();
    let id = read(&mut model, ":1.5", "A");
    model.signalled(":1.5", PATH, t(500));
    let same = model.read_done(&id, ":1.5", read_of(&id, "A"), t(500));
    assert_eq!(same.change, None);
    model.signalled(":1.5", PATH, t(1000));
    let done = model.read_done(&id, ":1.5", read_of(&id, "B"), t(1000));
    assert_eq!(done.change, Some(Change::Changed(model.item(&id).unwrap())));
}

#[test]
fn signals_during_a_read_ask_for_one_more_read_after_it() {
    let mut model = model();
    let id = register(&mut model, ":1.5");
    // The registration's read is in flight.
    assert_eq!(model.signalled(":1.5", PATH, t(500)), Vec::<Due>::new());
    assert_eq!(model.signalled(":1.5", PATH, t(500)), Vec::<Due>::new());
    let done = model.read_done(&id, ":1.5", Outcome::Failed, t(500));
    assert_eq!(
        done.next.map(|due| (due.id, due.at)),
        Some((id.clone(), t(500)))
    );
    let done = model.read_done(&id, ":1.5", read_of(&id, "A"), t(600));
    assert_eq!(done.next, None);
    // Idle again: the next signal starts a read.
    assert_eq!(model.signalled(":1.5", PATH, t(1000)).len(), 1);
}

#[test]
fn reads_of_one_item_start_no_closer_than_the_interval() {
    let mut model = model();
    let interval = Limits::default().min_read_interval;
    let id = register(&mut model, ":1.5");
    // Asked for while the first read, started at 0, is in flight.
    model.signalled(":1.5", PATH, t(10));
    let next = model.read_done(&id, ":1.5", read_of(&id, "A"), t(20)).next;
    assert_eq!(next.map(|due| due.at), Some(t(0) + interval));
    // A signal once idle, but within the interval of that start, waits too.
    let started = t(0) + interval;
    model.read_done(&id, ":1.5", Outcome::Failed, started);
    let due = model.signalled(":1.5", PATH, started + interval / 2);
    assert_eq!(due[0].at, started + interval);
    // One long after starts at once.
    model.read_done(&id, ":1.5", Outcome::Failed, started + interval);
    let later = started + interval * 10;
    assert_eq!(model.signalled(":1.5", PATH, later)[0].at, later);
}

#[test]
fn signals_while_a_read_waits_for_its_start_ask_for_nothing_more() {
    let mut model = model();
    let interval = Limits::default().min_read_interval;
    let id = register(&mut model, ":1.5");
    model.read_done(&id, ":1.5", read_of(&id, "A"), t(10));
    // Due at the interval; the burst comes before it starts, and that read
    // will see all of it.
    let due = model.signalled(":1.5", PATH, t(20));
    assert_eq!(due[0].at, t(0) + interval);
    for ms in 21..60 {
        assert_eq!(model.signalled(":1.5", PATH, t(ms)), Vec::<Due>::new());
    }
    let done = model.read_done(&id, ":1.5", Outcome::Failed, t(0) + interval * 2);
    assert_eq!(done.next, None);
}

#[test]
fn a_signal_reads_every_item_registered_at_its_object() {
    let mut model = model();
    let by_unique = model
        .register(target(":1.5"), ":1.5", t(0))
        .unwrap()
        .unwrap();
    let by_name = model
        .register(target("org.kde.StatusNotifierItem-1-1"), ":1.5", t(0))
        .unwrap()
        .unwrap();
    for due in [&by_unique, &by_name] {
        model.read_done(&due.id, ":1.5", read_of(&due.id, "A"), t(0));
    }
    let read: Vec<ItemId> = model
        .signalled(":1.5", PATH, t(500))
        .into_iter()
        .map(|due| due.id)
        .collect();
    assert_eq!(read, [by_unique.id, by_name.id]);
}

#[test]
fn signals_from_anything_but_a_registered_item_are_ignored() {
    let mut model = model();
    let id = read(&mut model, ":1.5", "A");
    assert_eq!(model.signalled(":1.6", PATH, t(500)), Vec::<Due>::new());
    assert_eq!(model.signalled(":1.5", "/Other", t(500)), Vec::<Due>::new());
    assert_eq!(model.signalled(":1.5", PATH, t(500)).len(), 1);
    assert_eq!(
        model.read_done(&id, ":1.5", Outcome::Failed, t(500)).next,
        None
    );
}

#[test]
fn an_item_whose_owner_is_gone_is_dropped() {
    let mut model = model();
    // Pending: the caller never heard of it.
    let pending = register(&mut model, ":1.5");
    let done = model.read_done(&pending, ":1.5", Outcome::Gone, t(0));
    assert!(done.unregistered);
    assert_eq!(done.change, None);
    // Reported: the caller hears it leave.
    let shown = read(&mut model, ":1.6", "A");
    model.signalled(":1.6", PATH, t(500));
    let done = model.read_done(&shown, ":1.6", Outcome::Gone, t(500));
    assert!(done.unregistered);
    assert_eq!(done.change, Some(Change::Removed(shown)));
    assert_eq!(model.registered(), Vec::<String>::new());
}

#[test]
fn a_read_from_an_owner_that_was_replaced_is_stale_not_gone() {
    let mut model = model();
    let service = "org.kde.StatusNotifierItem-1-1";
    let first = model
        .register(target(service), ":1.5", t(0))
        .unwrap()
        .unwrap();
    // The name moves while the read from :1.5 is in flight.
    let change = model.owner_changed(service, Some(":1.9"), t(10));
    assert_eq!(change.reads, Vec::<Due>::new());
    // :1.5 is gone, but it no longer owns the item: the read is stale.
    let done = model.read_done(&first.id, ":1.5", Outcome::Gone, t(20));
    assert!(!done.unregistered);
    // The read the move asked for follows, from the new owner.
    let next = done.next.unwrap();
    assert_eq!(next.id, first.id);
    assert_eq!(model.source(&next.id).unwrap().owner, ":1.9");
    assert_eq!(model.registered(), [first.id.to_string()]);
}

#[test]
fn a_read_waiting_for_its_start_goes_to_the_owner_at_the_start() {
    let mut model = model();
    let interval = Limits::default().min_read_interval;
    let service = "org.kde.StatusNotifierItem-1-1";
    let id = model
        .register(target(service), ":1.5", t(0))
        .unwrap()
        .unwrap()
        .id;
    // A read from :1.5 starts at 0; a signal during it asks for one more.
    model.signalled(":1.5", PATH, t(10));
    let next = model
        .read_done(&id, ":1.5", read_of(&id, "A"), t(20))
        .next
        .unwrap();
    assert_eq!(next.at, t(0) + interval);
    // The name moves while that read waits; the move asks for nothing more.
    let moved = model.owner_changed(service, Some(":1.9"), t(50));
    assert_eq!(moved.reads, Vec::<Due>::new());
    // When it starts, it goes to the new owner, and its result is kept.
    let source = model.source(&next.id).unwrap();
    assert_eq!(source.owner, ":1.9");
    let done = model.read_done(&id, &source.owner, read_of(&id, "B"), next.at);
    assert_eq!(done.change, Some(Change::Changed(model.item(&id).unwrap())));
    // Once the item has left, a waiting read has nobody to go to.
    model.owner_changed(service, None, next.at);
    assert_eq!(model.source(&id), None);
}

#[test]
fn a_service_losing_its_owner_takes_its_items_with_it() {
    let mut model = model();
    let pending = register(&mut model, ":1.5");
    let path = Target {
        service: ":1.5".into(),
        path: "/org/ayatana/NotificationItem/a".into(),
    };
    let shown = model.register(path, ":1.5", t(0)).unwrap().unwrap().id;
    model.read_done(&shown, ":1.5", read_of(&shown, "A"), t(0));
    let other = read(&mut model, ":1.6", "B");

    let change = model.owner_changed(":1.5", None, t(10));
    assert_eq!(change.unregistered, [pending, shown.clone()]);
    assert_eq!(change.changes, [Change::Removed(shown)]);
    assert_eq!(change.reads, Vec::<Due>::new());
    assert_eq!(model.registered(), [other.to_string()]);
}

#[test]
fn a_well_known_name_moving_to_another_owner_is_read_again_from_it() {
    let mut model = model();
    let service = "org.kde.StatusNotifierItem-1-1";
    let id = model
        .register(target(service), ":1.5", t(0))
        .unwrap()
        .unwrap()
        .id;
    model.read_done(&id, ":1.5", read_of(&id, "A"), t(0));
    let change = model.owner_changed(service, Some(":1.9"), t(500));
    let reads: Vec<ItemId> = change.reads.into_iter().map(|d| d.id).collect();
    assert_eq!(reads, std::slice::from_ref(&id));
    assert_eq!(model.source(&id).unwrap().owner, ":1.9");
    assert_eq!(change.unregistered, Vec::<ItemId>::new());
    // Its signals now come from the new owner.
    model.read_done(&id, ":1.9", Outcome::Failed, t(500));
    assert_eq!(model.signalled(":1.9", PATH, t(1000)).len(), 1);
    assert_eq!(model.signalled(":1.5", PATH, t(1000)), Vec::<Due>::new());
}

#[test]
fn hosts_are_registered_once_bounded_and_leave_with_their_name() {
    let mut model = Model::new(Limits {
        max_hosts: 1,
        ..Limits::default()
    });
    assert_eq!(model.register_host("org.example.Host"), Ok(true));
    assert_eq!(model.register_host("org.example.Host"), Ok(false));
    assert_eq!(
        model.register_host("org.example.Other"),
        Err(Refusal::Hosts)
    );
    let moved = model.owner_changed("org.example.Host", Some(":1.3"), t(0));
    assert_eq!(moved.hosts_left, 0);
    let left = model.owner_changed("org.example.Host", None, t(0));
    assert_eq!(left.hosts_left, 1);
    assert_eq!(model.register_host("org.example.Other"), Ok(true));
}

#[test]
fn reads_that_would_take_the_images_over_their_total_are_refused() {
    let mut model = Model::new(Limits {
        max_image_bytes_total: 8,
        ..Limits::default()
    });
    let with_pixels = |id: &ItemId, pixels: u32| {
        let mut item = item(id, "A");
        item.icon = Icon {
            name: None,
            pixmaps: vec![Arc::new(
                Image::new(pixels, 1, vec![0; pixels as usize * 4]).unwrap(),
            )],
        };
        Outcome::Read(item.into())
    };
    let a = register(&mut model, ":1.5");
    let done = model.read_done(&a, ":1.5", with_pixels(&a, 2), t(0));
    assert!(done.change.is_some());
    let b = register(&mut model, ":1.6");
    let done = model.read_done(&b, ":1.6", with_pixels(&b, 1), t(0));
    assert_eq!(done.change, None);
    assert_eq!(model.items().len(), 1);
    // Leaving frees its share.
    model.owner_changed(":1.5", None, t(10));
    assert_eq!(model.signalled(":1.6", PATH, t(500)).len(), 1);
    let done = model.read_done(&b, ":1.6", with_pixels(&b, 1), t(500));
    assert!(done.change.is_some());
}
