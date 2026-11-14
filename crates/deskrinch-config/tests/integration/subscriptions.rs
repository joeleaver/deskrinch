//! One watcher serving several sections.

use std::fs;
use std::os::unix::fs::symlink;
use std::sync::mpsc;

use deskrinch_config::{Event, Section, Watcher};
use deskrinch_test_wait::recv;

use crate::support::{Dirs, General, Panel, next_change, nothing, panel, subscribe, write};

#[test]
fn delivers_each_section_only_its_own_changes() {
    let dirs = Dirs::new();
    write(
        &dirs.user(),
        "[panel]\nheight = 40\n[general]\nterminal = 'kitty'\n",
    );
    let watcher = Watcher::start(dirs.sources(), |error| panic!("{error}")).unwrap();
    let (_panel, initial_panel, panel_events) = subscribe::<Panel>(&watcher, "panel");
    let (_general, initial_general, general_events) = subscribe::<General>(&watcher, "general");
    assert_eq!(initial_panel, panel(40));
    assert_eq!(initial_general.terminal, "kitty");

    write(
        &dirs.user(),
        "[panel]\nheight = 40\n[general]\nterminal = 'alacritty'\n",
    );
    assert_eq!(next_change(&general_events).terminal, "alacritty");
    nothing(&panel_events);

    write(
        &dirs.user(),
        "[panel]\nheight = 41\n[general]\nterminal = 'alacritty'\n",
    );
    assert_eq!(next_change(&panel_events), panel(41));
    nothing(&general_events);
}

#[test]
fn stops_delivering_once_a_subscription_is_dropped() {
    let dirs = Dirs::new();
    let watcher = Watcher::start(dirs.sources(), |error| panic!("{error}")).unwrap();
    let (kept, _, kept_events) = subscribe::<Panel>(&watcher, "panel");
    let (dropped, _, dropped_events) = subscribe::<Panel>(&watcher, "panel");
    drop(dropped);
    write(&dirs.user(), "[panel]\nheight = 50\n");
    assert_eq!(next_change(&kept_events), panel(50));
    // The sender went with the subscription: nothing more can arrive.
    assert!(matches!(dropped_events.recv(), Err(mpsc::RecvError)));
    drop(kept);
}

#[test]
fn lets_a_subscriber_subscribe_from_its_callback() {
    let dirs = Dirs::new();
    let watcher =
        std::sync::Arc::new(Watcher::start(dirs.sources(), |error| panic!("{error}")).unwrap());
    let (sender, events) = mpsc::channel();
    let inner = std::sync::Arc::downgrade(&watcher);
    let (_panel, _) = watcher.subscribe(Section::<Panel>::new("panel").unwrap(), move |event| {
        if let (Event::Changed(_), Some(watcher)) = (event, inner.upgrade()) {
            // Taking the lock again here must not deadlock.
            let section = Section::<General>::new("general").unwrap();
            let (subscription, general) = watcher.subscribe(section, |_| {});
            sender.send(general.settings).ok();
            drop(subscription);
        }
    });
    write(
        &dirs.user(),
        "[panel]\nheight = 50\n[general]\nterminal = 'kitty'\n",
    );
    let general = recv("the general settings", &events);
    assert_eq!(general.terminal, "kitty");
}

#[test]
fn follows_a_configuration_file_link_that_is_re_pointed() {
    let dirs = Dirs::new();
    let first = dirs.root.path().join("dotfiles/a.toml");
    let second = dirs.root.path().join("other/b.toml");
    write(&first, "[panel]\nheight = 40\n");
    write(&second, "[panel]\nheight = 60\n");
    fs::create_dir_all(dirs.user().parent().unwrap()).unwrap();
    symlink(&first, dirs.user()).unwrap();
    let (_watched, initial, events) = dirs.watch();
    assert_eq!(initial, panel(40));

    let replacement = dirs.user().with_file_name(".config.toml.new");
    symlink(&second, &replacement).unwrap();
    fs::rename(&replacement, dirs.user()).unwrap();
    assert_eq!(next_change(&events), panel(60));

    write(&second, "[panel]\nheight = 61\n");
    assert_eq!(next_change(&events), panel(61));
    // The old target no longer matters.
    write(&first, "[panel]\nheight = 41\n");
    nothing(&events);
}
