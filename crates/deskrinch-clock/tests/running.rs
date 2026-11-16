//! How many clocks run, and which worker a clock has: a test binary of its
//! own, so that no other test's clock is counted.

// Inside `cfg(test)`, where clippy allows the tests their `unwrap`s.
#[cfg(test)]
mod running {
    use std::path::PathBuf;

    use deskrinch_clock::{Clock, Environment, Precision, running};

    fn start() -> Clock {
        let env = Environment {
            tz: Some("UTC".to_owned()),
            zoneinfo: PathBuf::from("/usr/share/zoneinfo"),
            localtime: PathBuf::from("/etc/localtime"),
            time_locale: None,
        };
        Clock::start(env, Precision::Minutes, |_| {}).unwrap()
    }

    /// A clock counts from its start until its drop returns, at once both
    /// ways; each clock has a worker of its own, and a clock started after
    /// another has ended has another worker still.
    #[test]
    fn counts_each_clock_from_its_start_until_its_drop() {
        assert_eq!(running(), 0);
        let first = start();
        assert_eq!(running(), 1);
        let second = start();
        assert_eq!(running(), 2);
        assert_ne!(first.worker(), second.worker());

        let ended = first.worker();
        drop(first);
        assert_eq!(running(), 1);
        let third = start();
        assert_eq!(running(), 2);
        assert_ne!(third.worker(), ended);

        drop(second);
        drop(third);
        assert_eq!(running(), 0);
    }
}
