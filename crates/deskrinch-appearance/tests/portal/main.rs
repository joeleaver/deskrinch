//! The appearance against a stand-in settings portal on a private bus
//! (`deskrinch-test-dbus`), never the session's.

#[cfg(test)]
mod support;

#[cfg(test)]
mod follow {
    use deskrinch_appearance::{AppearanceError, ColorScheme, Event};
    use zbus::OwnedVariant;

    use crate::support::{StandIn, bus, follow, next, number, quiet};

    fn scheme(event: Event) -> ColorScheme {
        match event {
            Event::ColorScheme(scheme) => scheme,
            other => panic!("expected a colour scheme, got {other:?}"),
        }
    }

    #[test]
    fn reads_the_scheme_and_follows_changes() {
        let bus = bus();
        let portal = StandIn::start(&bus, Some(number(1)), false);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
        portal.set(number(2));
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
        portal.set(number(0));
        assert_eq!(scheme(next(&events)), ColorScheme::NoPreference);
        quiet(&events);
    }

    #[test]
    fn no_portal_then_one_that_appears() {
        let bus = bus();
        let (_appearance, events) = follow(&bus);
        assert!(matches!(next(&events), Event::NoPortal));
        let _portal = StandIn::start(&bus, Some(number(2)), false);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn a_portal_that_goes_away_and_comes_back() {
        let bus = bus();
        let portal = StandIn::start(&bus, Some(number(1)), false);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
        portal.stop();
        assert!(matches!(next(&events), Event::NoPortal));
        let _again = StandIn::start(&bus, Some(number(2)), false);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn a_value_it_does_not_know() {
        let bus = bus();
        let portal = StandIn::start(&bus, Some(number(7)), false);
        let (_appearance, events) = follow(&bus);
        // The specification: an unknown value is no preference.
        assert_eq!(scheme(next(&events)), ColorScheme::NoPreference);
        // Not a number at all: reported, and following carries on.
        portal.set(OwnedVariant::try_from(zbus::Variant::from("dark")).unwrap());
        assert!(matches!(
            next(&events),
            Event::Error(AppearanceError::UnexpectedValue { found }) if found == "s"
        ));
        portal.set(number(1));
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
    }

    #[test]
    fn a_portal_without_the_key_has_no_preference() {
        let bus = bus();
        let _portal = StandIn::start(&bus, None, false);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::NoPreference);
    }

    #[test]
    fn an_older_portal_with_read_only() {
        let bus = bus();
        let _portal = StandIn::start(&bus, Some(number(2)), true);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferLight);
    }

    #[test]
    fn ignores_a_setting_changed_from_anyone_but_the_portal() {
        let bus = bus();
        let portal = StandIn::start(&bus, Some(number(1)), false);
        let (_appearance, events) = follow(&bus);
        assert_eq!(scheme(next(&events)), ColorScheme::PreferDark);
        let impostor =
            zbus::block_on(zbus::connection::Builder::address(bus.address()).build()).unwrap();
        zbus::block_on(impostor.emit_signal(
            None::<&str>,
            crate::support::PATH,
            crate::support::SETTINGS,
            "SettingChanged",
            &(
                crate::support::NAMESPACE,
                crate::support::KEY,
                zbus::Variant::from(2_u32),
            ),
        ))
        .unwrap();
        quiet(&events);
        portal.set(number(2));
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
        let _portal = StandIn::start(&bus, Some(number(1)), false);
        quiet(&events);
    }
}

#[cfg(test)]
mod activation {
    use std::time::Duration;

    use deskrinch_appearance::{ColorScheme, Event};

    use crate::support::{StandIn, activating_bus, bus, follow, next, number, quiet};

    /// The stand-in portal the bus starts; not a test of its own.
    ///
    /// It answers `STAND_IN_SCHEME`. With `STAND_IN_FILE` it answers the
    /// number in that file instead, and changes when the file does (checked
    /// ten times a second, for up to an hour): what a manual check of a
    /// component in an isolated session runs, with `DBUS_STARTER_ADDRESS`
    /// naming the session's bus.
    #[test]
    #[ignore = "run by dbus-daemon when it activates the stand-in portal"]
    fn stand_in() {
        let Ok(address) = std::env::var("DBUS_STARTER_ADDRESS") else {
            return;
        };
        if let Some(path) = std::env::var_os("STAND_IN_FILE") {
            let read =
                || -> Option<u32> { std::fs::read_to_string(&path).ok()?.trim().parse().ok() };
            let mut shown = read().unwrap_or(0);
            let portal = StandIn::start_at(&address, Some(number(shown)), false);
            for _ in 0..36_000 {
                std::thread::sleep(Duration::from_millis(100));
                if let Some(now) = read().filter(|now| *now != shown) {
                    portal.set(number(now));
                    shown = now;
                }
            }
            return;
        }
        let Ok(scheme) = std::env::var("STAND_IN_SCHEME") else {
            return;
        };
        let _portal = StandIn::start_at(&address, Some(number(scheme.parse().unwrap())), false);
        // Until the bus ends it, with the rest of its process group.
        std::thread::sleep(Duration::from_secs(60));
    }

    #[test]
    fn starts_a_portal_nothing_has_started() {
        let (bus, _services) = activating_bus(2);
        let (_appearance, events) = follow(&bus);
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
