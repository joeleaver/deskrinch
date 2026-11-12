//! The launcher's interface and its single instance, on a private bus
//! (`deskrinch-test-dbus`), never the session's.

#[cfg(test)]
mod launcher {
    use std::sync::Arc;

    use deskrinch_bus::Bus;
    use deskrinch_launcher::{
        Answer, INTERFACE, NAME, PATH, Request, ServeError, Service, Visibility, send,
    };
    use deskrinch_test_dbus::PrivateBus;

    fn bus() -> (PrivateBus, Bus) {
        let private = PrivateBus::start().expect("the launcher's tests need dbus-daemon");
        let bus = Bus::Address(private.address().to_owned());
        (private, bus)
    }

    fn done(shown: Option<bool>) -> Answer {
        Answer::Done { shown }
    }

    #[test]
    fn toggle_show_and_hide_reach_the_running_launcher() {
        let (_private, bus) = bus();
        let visibility = Arc::new(Visibility::new(false));
        let _service = Service::start(&bus, Arc::clone(&visibility)).unwrap();

        assert_eq!(send(&bus, Request::Toggle).unwrap(), done(Some(true)));
        assert!(visibility.is_shown());
        assert_eq!(send(&bus, Request::Toggle).unwrap(), done(Some(false)));
        assert!(!visibility.is_shown());

        assert_eq!(send(&bus, Request::Show).unwrap(), done(None));
        assert_eq!(send(&bus, Request::Show).unwrap(), done(None));
        assert!(visibility.is_shown(), "a second show leaves it shown");
        assert_eq!(send(&bus, Request::Hide).unwrap(), done(None));
        assert!(!visibility.is_shown());

        // Hidden by its own UI, a toggle shows it again.
        visibility.show();
        visibility.hide();
        assert_eq!(send(&bus, Request::Toggle).unwrap(), done(Some(true)));
    }

    /// The launcher's `Shown` property, read on `bus`.
    fn shown(bus: &Bus) -> bool {
        zbus::block_on(async {
            let connection = bus.connect().await.unwrap();
            let properties = zbus::fdo::PropertiesProxy::builder(&connection)
                .destination(NAME)
                .path(PATH)
                .build()
                .await
                .unwrap();
            let interface = zbus::names::InterfaceName::try_from(INTERFACE).unwrap();
            let value = properties.get(interface, "Shown").await.unwrap();
            bool::try_from(value).unwrap()
        })
    }

    #[test]
    fn the_shown_property_follows_every_change() {
        let (_private, bus) = bus();
        let visibility = Arc::new(Visibility::new(false));
        let _service = Service::start(&bus, Arc::clone(&visibility)).unwrap();
        assert!(!shown(&bus));
        send(&bus, Request::Toggle).unwrap();
        assert!(shown(&bus));
        // Hidden by its own UI.
        visibility.hide();
        assert!(!shown(&bus));
    }

    /// The shell starts the launcher and waits for it by the name the shell's
    /// description gives; it must be the one the launcher takes.
    #[test]
    fn owns_the_name_the_shell_waits_for() {
        let launcher = deskrinch_shell::COMPONENTS
            .iter()
            .find(|component| component.package == env!("CARGO_PKG_NAME"))
            .unwrap();
        assert_eq!(launcher.ready, deskrinch_shell::Ready::BusName(NAME));
        assert_eq!(
            deskrinch_shell::LAUNCHER_TOGGLE.package,
            env!("CARGO_PKG_NAME")
        );
        assert_eq!(deskrinch_shell::LAUNCHER_TOGGLE.args, ["toggle"]);
    }

    #[test]
    fn a_second_launcher_finds_the_first() {
        let (_private, bus) = bus();
        let first = Arc::new(Visibility::new(false));
        let _service = Service::start(&bus, Arc::clone(&first)).unwrap();
        let second = Service::start(&bus, Arc::new(Visibility::new(false)));
        assert!(matches!(second, Err(ServeError::Running)), "{second:?}");
        // The first still answers.
        assert_eq!(send(&bus, Request::Show).unwrap(), done(None));
        assert!(first.is_shown());
    }

    #[test]
    fn with_no_launcher_a_request_says_so() {
        let (_private, bus) = bus();
        for request in [Request::Toggle, Request::Show, Request::Hide] {
            assert_eq!(
                send(&bus, request).unwrap(),
                Answer::NotRunning,
                "{request:?}"
            );
        }
    }

    #[test]
    fn a_launcher_that_ends_gives_its_name_back() {
        let (_private, bus) = bus();
        let service = Service::start(&bus, Arc::new(Visibility::new(false))).unwrap();
        drop(service);
        assert_eq!(send(&bus, Request::Toggle).unwrap(), Answer::NotRunning);
        let again = Arc::new(Visibility::new(false));
        let _service = Service::start(&bus, Arc::clone(&again)).unwrap();
        assert_eq!(send(&bus, Request::Toggle).unwrap(), done(Some(true)));
        assert!(again.is_shown());
    }

    #[test]
    fn an_unreachable_bus_is_an_error_not_no_launcher() {
        let bus = Bus::Address("unix:path=/nonexistent/bus".into());
        assert!(send(&bus, Request::Toggle).is_err());
        assert!(matches!(
            Service::start(&bus, Arc::new(Visibility::new(false))),
            Err(ServeError::Bus(_))
        ));
    }
}
