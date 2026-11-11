use deskrinch_untrusted::Image;

use super::*;
use crate::item::Icon;
use crate::parse;
use crate::wire::Properties;

fn target(service: &str) -> Target {
    Target {
        service: service.into(),
        path: "/StatusNotifierItem".into(),
    }
}

fn model() -> Model {
    Model::new(Limits::default())
}

/// The item `id` titled `title`.
fn item(id: &ItemId, title: &str) -> Item {
    let properties = Properties {
        title: Some(title.into()),
        ..Properties::default()
    };
    parse::item(id.clone(), properties, &Limits::default())
}

/// Registers `service`, owned by itself; the id.
fn register(model: &mut Model, service: &str) -> ItemId {
    model.register(target(service), service).unwrap().unwrap().0
}

/// Registers `service` and reads it as titled `title`; the id.
fn read(model: &mut Model, service: &str, title: &str) -> ItemId {
    let id = register(model, service);
    let done = model.read_done(&id, Outcome::Read(item(&id, title).into()));
    assert!(matches!(done.change, Some(Change::Added(_))));
    id
}

#[test]
fn a_registered_item_is_listed_but_pending_until_it_is_read() {
    let mut model = model();
    let (id, read) = model.register(target(":1.5"), ":1.5").unwrap().unwrap();
    assert_eq!(id.as_str(), ":1.5/StatusNotifierItem");
    assert_eq!(read, target(":1.5"));
    assert_eq!(model.registered(), [":1.5/StatusNotifierItem"]);
    assert_eq!(model.items(), Vec::<Arc<Item>>::new());
    assert_eq!(model.target(&id), None);

    let done = model.read_done(&id, Outcome::Read(item(&id, "A").into()));
    assert_eq!(done.change, Some(Change::Added(model.item(&id).unwrap())));
    assert_eq!(model.items().len(), 1);
    assert_eq!(model.target(&id), Some(target(":1.5")));
}

#[test]
fn registering_again_changes_nothing() {
    let mut model = model();
    register(&mut model, ":1.5");
    assert_eq!(model.register(target(":1.5"), ":1.5"), Ok(None));
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
    model.register(path("/a"), ":1.1").unwrap();
    model.register(path("/b"), ":1.1").unwrap();
    assert_eq!(
        model.register(path("/c"), ":1.1"),
        Err(Refusal::PerConnection)
    );
    register(&mut model, ":1.2");
    assert_eq!(model.register(target(":1.3"), ":1.3"), Err(Refusal::Items));
}

#[test]
fn a_failed_read_leaves_a_pending_item_pending_and_a_read_one_as_it_was() {
    let mut model = model();
    let pending = register(&mut model, ":1.5");
    assert_eq!(
        model.read_done(&pending, Outcome::Failed),
        ReadDone::default()
    );
    assert_eq!(model.items(), Vec::<Arc<Item>>::new());

    let id = read(&mut model, ":1.6", "A");
    model.signalled(":1.6", "/StatusNotifierItem").unwrap();
    assert_eq!(model.read_done(&id, Outcome::Failed), ReadDone::default());
    assert_eq!(model.item(&id).unwrap().title, "A");
}

#[test]
fn a_later_read_reports_a_change_only_when_something_changed() {
    let mut model = model();
    let id = read(&mut model, ":1.5", "A");
    model.signalled(":1.5", "/StatusNotifierItem").unwrap();
    assert_eq!(
        model
            .read_done(&id, Outcome::Read(item(&id, "A").into()))
            .change,
        None
    );
    model.signalled(":1.5", "/StatusNotifierItem").unwrap();
    let done = model.read_done(&id, Outcome::Read(item(&id, "B").into()));
    assert_eq!(done.change, Some(Change::Changed(model.item(&id).unwrap())));
}

#[test]
fn signals_during_a_read_ask_for_one_more_read_after_it() {
    let mut model = model();
    let id = register(&mut model, ":1.5");
    // The registration's read is in flight.
    assert_eq!(model.signalled(":1.5", "/StatusNotifierItem"), None);
    assert_eq!(model.signalled(":1.5", "/StatusNotifierItem"), None);
    let done = model.read_done(&id, Outcome::Failed);
    assert_eq!(done.next, Some((id.clone(), target(":1.5"))));
    let done = model.read_done(&id, Outcome::Read(item(&id, "A").into()));
    assert_eq!(done.next, None);
    // Idle again: the next signal starts a read.
    assert_eq!(
        model.signalled(":1.5", "/StatusNotifierItem"),
        Some((id, target(":1.5")))
    );
}

#[test]
fn signals_from_anything_but_a_registered_item_are_ignored() {
    let mut model = model();
    let id = read(&mut model, ":1.5", "A");
    assert_eq!(model.signalled(":1.6", "/StatusNotifierItem"), None);
    assert_eq!(model.signalled(":1.5", "/Other"), None);
    assert!(model.signalled(":1.5", "/StatusNotifierItem").is_some());
    assert_eq!(model.read_done(&id, Outcome::Failed).next, None);
}

#[test]
fn an_item_gone_before_its_read_is_dropped_unannounced_to_the_caller() {
    let mut model = model();
    let id = register(&mut model, ":1.5");
    let done = model.read_done(&id, Outcome::Gone);
    assert!(done.unregistered);
    assert_eq!(done.change, None);
    assert_eq!(model.registered(), Vec::<String>::new());
}

#[test]
fn a_service_losing_its_owner_takes_its_items_with_it() {
    let mut model = model();
    let pending = register(&mut model, ":1.5");
    let path = Target {
        service: ":1.5".into(),
        path: "/org/ayatana/NotificationItem/a".into(),
    };
    let (shown, _) = model.register(path, ":1.5").unwrap().unwrap();
    model.read_done(&shown, Outcome::Read(item(&shown, "A").into()));
    let other = read(&mut model, ":1.6", "B");

    let change = model.owner_changed(":1.5", None);
    assert_eq!(change.unregistered, [pending, shown.clone()]);
    assert_eq!(change.changes, [Change::Removed(shown)]);
    assert_eq!(change.reads, Vec::<(ItemId, Target)>::new());
    assert_eq!(model.registered(), [other.to_string()]);
}

#[test]
fn a_well_known_name_moving_to_another_owner_is_read_again_from_it() {
    let mut model = model();
    let service = "org.kde.StatusNotifierItem-1-1";
    let (id, _) = model.register(target(service), ":1.5").unwrap().unwrap();
    model.read_done(&id, Outcome::Read(item(&id, "A").into()));
    let change = model.owner_changed(service, Some(":1.9"));
    assert_eq!(change.reads, [(id.clone(), target(service))]);
    assert_eq!(change.unregistered, Vec::<ItemId>::new());
    // Its signals now come from the new owner.
    model.read_done(&id, Outcome::Failed);
    assert!(model.signalled(":1.9", "/StatusNotifierItem").is_some());
    assert_eq!(model.signalled(":1.5", "/StatusNotifierItem"), None);
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
    assert_eq!(
        model
            .owner_changed("org.example.Host", Some(":1.3"))
            .hosts_left,
        0
    );
    assert_eq!(model.owner_changed("org.example.Host", None).hosts_left, 1);
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
        item
    };
    let a = register(&mut model, ":1.5");
    assert!(
        model
            .read_done(&a, Outcome::Read(with_pixels(&a, 2).into()))
            .change
            .is_some()
    );
    let b = register(&mut model, ":1.6");
    assert_eq!(
        model
            .read_done(&b, Outcome::Read(with_pixels(&b, 1).into()))
            .change,
        None
    );
    assert_eq!(model.items().len(), 1);
    // Leaving frees its share.
    model.owner_changed(":1.5", None);
    model.signalled(":1.6", "/StatusNotifierItem").unwrap();
    assert!(
        model
            .read_done(&b, Outcome::Read(with_pixels(&b, 1).into()))
            .change
            .is_some()
    );
}
