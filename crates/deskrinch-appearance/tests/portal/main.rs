//! The appearance against a stand-in settings portal on a private bus
//! (`deskrinch-test-portal` on `deskrinch-test-dbus`), never the session's.

#[cfg(test)]
mod support;

#[cfg(test)]
mod follow {
    use deskrinch_appearance::{AppearanceError, ColorScheme, Event};
    use deskrinch_test_portal::Methods;
    use zbus::OwnedVariant;

    use crate::support::{bus, follow, next, number, portal as stand_in, quiet};

    fn scheme(event: Event) -> ColorScheme {
        match event {
            Event::ColorScheme(scheme) => scheme,
            other => panic!("expected a colour scheme, got {other:?}"),
        }
    }

    #[test]
    fn reads_the_scheme_and_follows_changes() {
        let bus = bus();
        let portal = stand_in(&bus, Some(number(1)), Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
        portal.set(number(2)).unwrap();
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
        portal.set(number(0)).unwrap();
        assert_eq!(scheme(next(&events)), ColorScheme::NoPreference);
        quiet(&events);
    }

    #[test]
    fn no_portal_then_one_that_appears() {
        let bus = bus();
        let (_appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        let _portal = stand_in(&bus, Some(number(2)), Methods::Current);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn a_portal_that_goes_away_and_comes_back() {
        let bus = bus();
        let portal = stand_in(&bus, Some(number(1)), Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
        portal.stop().unwrap();
        assert!(matches!(next(&events), Event::NoPortal));
        let _again = stand_in(&bus, Some(number(2)), Methods::Current);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn a_value_it_does_not_know() {
        let bus = bus();
        let portal = stand_in(&bus, Some(number(7)), Methods::Current);
        let (_appearance, events) = follow(&bus);
        // The specification: an unknown value is no preference.
        assert_eq!(scheme(next(&events)), ColorScheme::NoPreference);
        // Not a number at all: reported, and following carries on.
        portal
            .set(OwnedVariant::try_from(zbus::Variant::from("dark")).unwrap())
            .unwrap();
        assert!(matches!(
            next(&events),
            Event::Error(AppearanceError::UnexpectedValue { found }) if found == "s"
        ));
        portal.set(number(1)).unwrap();
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
    }

    #[test]
    fn a_portal_without_the_key_has_no_preference() {
        let bus = bus();
        let _portal = stand_in(&bus, None, Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::NoPreference);
    }

    #[test]
    fn an_older_portal_with_read_only() {
        let bus = bus();
        let _portal = stand_in(&bus, Some(number(2)), Methods::ReadOnly);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn ignores_a_setting_changed_from_anyone_but_the_portal() {
        let bus = bus();
        let portal = stand_in(&bus, Some(number(1)), Methods::Current);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
        let impostor =
            zbus::block_on(zbus::connection::Builder::address(bus.address()).build()).unwrap();
        zbus::block_on(impostor.emit_signal(
            None::<&str>,
            crate::support::PATH,
            crate::support::INTERFACE,
            "SettingChanged",
            &(
                crate::support::NAMESPACE,
                crate::support::KEY,
                zbus::Variant::from(2_u32),
            ),
        ))
        .unwrap();
        quiet(&events);
        portal.set(number(2)).unwrap();
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn stops_when_dropped() {
        let bus = bus();
        let (appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        let start = std::time::Instant::now();
        drop(appearance);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        let _portal = stand_in(&bus, Some(number(1)), Methods::Current);
        quiet(&events);
    }
}

#[cfg(test)]
mod activation {
    use deskrinch_appearance::{ColorScheme, Event};

    use crate::support::{activating_bus, bus, follow, next, quiet};

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
        let bus = activating_bus(2);
        let (_appearance, events) = follow(bus.bus());
        assert!(
            matches!(next(&events), Event::ColorScheme(ColorScheme::PreferLight)),
            "the bus started the stand-in and it was read"
        );
        // Its owner change does not repeat it.
        quiet(&events);
    }

    #[test]
    fn a_bus_that_cannot_start_one_has_no_portal() {
        let bus = bus();
        let (_appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        quiet(&events);
    }
}
