//! The appearance against a stand-in settings portal on a private bus
//! (`deskrinch-test-portal` on `deskrinch-test-dbus`), never the session's.

#[cfg(test)]
mod support;

#[cfg(test)]
mod follow {
    use deskrinch_appearance::{AppearanceError, ColorScheme, Event, IconThemes, Preferences};
    use deskrinch_test_portal::{
        COLOR_SCHEME, GNOME_ICON_THEME, INTERFACE, KDE_ICON_THEME, Methods, PATH, Setting, Value,
    };

    use crate::support::{bus, follow, next, portal as stand_in, preferences};

    fn scheme(number: u32) -> [(Setting<'static>, Value); 1] {
        [(COLOR_SCHEME, Value::Number(number))]
    }

    #[test]
    fn reads_the_scheme_and_follows_changes() {
        let bus = bus();
        let portal = stand_in(&bus, &scheme(1), Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferDark);
        portal.set(COLOR_SCHEME, Value::Number(2)).unwrap();
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferLight);
        portal.set(COLOR_SCHEME, Value::Number(0)).unwrap();
        assert_eq!(preferences(&events).color_scheme, ColorScheme::NoPreference);
        // The same value again changes nothing, and is not passed on.
        // Sentinel: the portal's changes to one key arrive in order, so a
        // new value's preferences come first.
        portal.set(COLOR_SCHEME, Value::Number(0)).unwrap();
        portal.set(COLOR_SCHEME, Value::Number(1)).unwrap();
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferDark);
    }

    #[test]
    fn reads_both_icon_theme_keys_and_follows_them() {
        let bus = bus();
        let portal = stand_in(
            &bus,
            &[
                (KDE_ICON_THEME, Value::text("breeze")),
                (GNOME_ICON_THEME, Value::text("Adwaita")),
            ],
            Methods::Current,
        );
        let (_appearance, events) = follow(&bus);
        let first = preferences(&events);
        assert_eq!(
            first.icon_themes,
            IconThemes {
                kde: Some("breeze".into()),
                gnome: Some("Adwaita".into()),
            }
        );
        // No colour-scheme key: no preference.
        assert_eq!(first.color_scheme, ColorScheme::NoPreference);
        portal.set(KDE_ICON_THEME, Value::text("oxygen")).unwrap();
        assert_eq!(
            preferences(&events).icon_themes.kde.as_deref(),
            Some("oxygen")
        );
        portal.set(GNOME_ICON_THEME, Value::text("")).unwrap();
        assert_eq!(preferences(&events).icon_themes.gnome, None);
        // Another key in a namespace that is read, and a namespace that is
        // not: neither is passed on.
        let other_key = Setting {
            namespace: KDE_ICON_THEME.namespace,
            key: "Size",
        };
        portal.set(other_key, Value::Number(22)).unwrap();
        let other_namespace = Setting {
            namespace: "org.kde.kdeglobals.General",
            key: "ColorScheme",
        };
        portal
            .set(other_namespace, Value::text("BreezeDark"))
            .unwrap();
        // Sentinel: the portal's changes in one namespace arrive in order
        // (and one the crate does not read never reaches it), so a change
        // to a key it reads, in that namespace, comes first.
        portal.set(KDE_ICON_THEME, Value::text("breeze")).unwrap();
        assert_eq!(
            preferences(&events).icon_themes.kde.as_deref(),
            Some("breeze")
        );
    }

    #[test]
    fn no_portal_then_one_that_appears() {
        let bus = bus();
        let (_appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        let _portal = stand_in(&bus, &scheme(2), Methods::Current);
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferLight);
    }

    #[test]
    fn a_portal_that_goes_away_and_comes_back() {
        let bus = bus();
        let portal = stand_in(&bus, &scheme(1), Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferDark);
        portal.stop().unwrap();
        assert!(matches!(next(&events), Event::NoPortal));
        let _again = stand_in(&bus, &scheme(2), Methods::Current);
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferLight);
    }

    #[test]
    fn a_value_it_does_not_know() {
        let bus = bus();
        let portal = stand_in(&bus, &scheme(7), Methods::Current);
        let (_appearance, events) = follow(&bus);
        // The specification: an unknown value is no preference.
        assert_eq!(preferences(&events).color_scheme, ColorScheme::NoPreference);
        // Not a number at all: reported, and following carries on.
        portal.set(COLOR_SCHEME, Value::text("dark")).unwrap();
        assert!(matches!(
            next(&events),
            Event::Error(AppearanceError::UnexpectedValue { found, .. }) if found == "s"
        ));
        portal.set(COLOR_SCHEME, Value::Number(1)).unwrap();
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferDark);
    }

    #[test]
    fn a_value_of_the_wrong_type_at_the_start_is_reported_then_the_rest() {
        let bus = bus();
        let _portal = stand_in(
            &bus,
            &[
                (KDE_ICON_THEME, Value::Number(1)),
                (GNOME_ICON_THEME, Value::text("Adwaita")),
            ],
            Methods::Current,
        );
        let (_appearance, events) = follow(&bus);
        assert!(matches!(
            next(&events),
            Event::Error(AppearanceError::UnexpectedValue {
                expected: "string",
                ..
            })
        ));
        let icon_themes = preferences(&events).icon_themes;
        assert_eq!(icon_themes.kde, None);
        assert_eq!(icon_themes.gnome.as_deref(), Some("Adwaita"));
    }

    #[test]
    fn a_portal_without_the_keys_has_the_defaults() {
        let bus = bus();
        let _portal = stand_in(&bus, &[], Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(preferences(&events), Preferences::default());
    }

    #[test]
    fn an_older_portal_with_read_only() {
        let bus = bus();
        let _portal = stand_in(
            &bus,
            &[
                (COLOR_SCHEME, Value::Number(2)),
                (KDE_ICON_THEME, Value::text("breeze")),
            ],
            Methods::ReadOnly,
        );
        let (_appearance, events) = follow(&bus);
        let first = preferences(&events);
        assert_eq!(first.color_scheme, ColorScheme::PreferLight);
        assert_eq!(first.icon_themes.kde.as_deref(), Some("breeze"));
    }

    #[test]
    fn ignores_a_setting_changed_from_anyone_but_the_portal() {
        let bus = bus();
        let portal = stand_in(&bus, &scheme(1), Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferDark);
        let impostor =
            zbus::block_on(zbus::connection::Builder::address(bus.address()).build()).unwrap();
        zbus::block_on(impostor.emit_signal(
            None::<&str>,
            PATH,
            INTERFACE,
            "SettingChanged",
            &(
                COLOR_SCHEME.namespace,
                COLOR_SCHEME.key,
                zbus::Variant::from(2_u32),
            ),
        ))
        .unwrap();
        // Sentinel: the bus routes each message as it reads it, so once the
        // impostor's own call to the bus is answered, its signal is queued
        // to the follower ahead of anything the portal sends after; a value
        // other than the impostor's then comes first.
        zbus::block_on(impostor.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetId",
            &(),
        ))
        .unwrap();
        portal.set(COLOR_SCHEME, Value::Number(0)).unwrap();
        assert_eq!(preferences(&events).color_scheme, ColorScheme::NoPreference);
    }

    #[test]
    fn stops_when_dropped() {
        let bus = bus();
        let (appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        // The drop returns: it does not wait on a bus that has no portal.
        let (dropped, done) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(appearance);
            dropped.send(()).ok();
        });
        let () = deskrinch_test_wait::recv("the drop to return", &done);
        let _portal = stand_in(&bus, &scheme(1), Methods::Current);
        // Sentinel: the drop joined the thread, which owned the subscriber,
        // so the channel is closed and nothing more can come.
        match events.recv_timeout(deskrinch_test_wait::PATIENCE) {
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
            other => panic!("expected the subscriber gone, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod activation {
    use deskrinch_appearance::{ColorScheme, Event};
    use deskrinch_test_portal::{COLOR_SCHEME, KDE_ICON_THEME, Methods, Value};

    use crate::support::{activating_bus, bus, follow, next, portal, preferences, quiet};

    /// The stand-in portal the bus starts (`deskrinch_test_portal`'s
    /// `serve_from_env`); not a test of its own.
    #[test]
    #[ignore = "run by dbus-daemon when it activates the stand-in portal"]
    fn stand_in() {
        if std::env::var_os("DBUS_STARTER_ADDRESS").is_none() {
            return;
        }
        deskrinch_test_portal::serve_from_env(|name| std::env::var_os(name)).unwrap();
    }

    #[test]
    fn starts_a_portal_nothing_has_started() {
        let bus = activating_bus(&[
            (COLOR_SCHEME, Value::Number(2)),
            (KDE_ICON_THEME, Value::text("breeze")),
        ]);
        let (_appearance, events) = follow(bus.bus());
        // The bus started the stand-in, and it was read.
        let first = preferences(&events);
        assert_eq!(first.color_scheme, ColorScheme::PreferLight);
        assert_eq!(first.icon_themes.kde.as_deref(), Some("breeze"));
        // Its owner change does not repeat it. A window: the stand-in the bus
        // started has no handle here to make a change that could follow as
        // a sentinel.
        quiet(&events);
    }

    #[test]
    fn a_bus_that_cannot_start_one_has_no_portal() {
        let bus = bus();
        let (_appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        // Nothing more. Sentinel: the bus's signals arrive in order, so a
        // portal that appears now is read and reported first.
        let _portal = portal(&bus, &[(COLOR_SCHEME, Value::Number(1))], Methods::Current);
        assert_eq!(preferences(&events).color_scheme, ColorScheme::PreferDark);
    }
}
