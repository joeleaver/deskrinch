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
