//! Whether a process runs inside an isolated session: the check, from the
//! three values a session gives its clients, each one required.

use std::ffi::OsString;
use std::fmt;
use std::path::Path;

use crate::root::{self, NotARoot};
use crate::{BUS_VARIABLE, COMPOSITOR_VARIABLE, SESSION_DIR_VARIABLE, bus};

/// What the check reads: the session's three variables, as the process has
/// them. Built from the environment ([`Environment::read`]) or, in a test of
/// a refusal, by hand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// [`COMPOSITOR_VARIABLE`].
    pub compositor: Option<OsString>,
    /// [`SESSION_DIR_VARIABLE`].
    pub session_dir: Option<OsString>,
    /// [`BUS_VARIABLE`].
    pub bus: Option<OsString>,
}

impl Environment {
    /// The process's own.
    #[must_use]
    pub fn read() -> Self {
        Self {
            compositor: std::env::var_os(COMPOSITOR_VARIABLE),
            session_dir: std::env::var_os(SESSION_DIR_VARIABLE),
            bus: std::env::var_os(BUS_VARIABLE),
        }
    }
}

/// A process inside an isolated session: which compositor the session
/// runs, as the session names it, and the bus the check found a private
/// one of ours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isolated {
    /// The session's compositor's name: `kwin`, `sway` or `labwc`.
    pub compositor: String,
    /// The session's bus's address, checked ([`bus::is_private`]).
    pub bus: String,
}

/// Why a process is not inside an isolated session: the first of the three
/// conditions it fails, named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotIsolated {
    /// [`COMPOSITOR_VARIABLE`] is not set, or empty, or not text.
    NoCompositor,
    /// [`SESSION_DIR_VARIABLE`] is not set.
    NoSessionDir,
    /// [`SESSION_DIR_VARIABLE`] names no session's root.
    NotASessionRoot(NotARoot),
    /// [`BUS_VARIABLE`] is not set, or not text.
    NoBus,
    /// [`BUS_VARIABLE`] names a bus that is not a private one of ours: the
    /// real session's, most likely.
    NotAPrivateBus {
        /// The address it names.
        address: String,
    },
}

impl fmt::Display for NotIsolated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not inside an isolated session (run it with `cargo xtask session` or `cargo xtask compositor-tests`): ")?;
        match self {
            Self::NoCompositor => {
                write!(f, "{COMPOSITOR_VARIABLE} is not set, or empty, or not text")
            }
            Self::NoSessionDir => write!(f, "{SESSION_DIR_VARIABLE} is not set"),
            Self::NotASessionRoot(not) => write!(f, "{SESSION_DIR_VARIABLE}: {not}"),
            Self::NoBus => write!(f, "{BUS_VARIABLE} is not set, or not text"),
            Self::NotAPrivateBus { address } => {
                write!(
                    f,
                    "{BUS_VARIABLE} is {address}, not a session's private bus"
                )
            }
        }
    }
}

impl std::error::Error for NotIsolated {}

/// Whether `environment` is that of a process inside an isolated session:
/// it names the session's compositor, a session's root directory
/// ([`root::check`]), and a private bus of ours ([`bus::is_private`]).
/// Checked before anything touches a bus, so that a refusal reaches
/// nothing.
///
/// # Errors
///
/// [`NotIsolated`], naming the first condition that fails.
pub fn check(environment: &Environment) -> Result<Isolated, NotIsolated> {
    let compositor = environment
        .compositor
        .as_ref()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or(NotIsolated::NoCompositor)?;
    let root = environment
        .session_dir
        .as_ref()
        .ok_or(NotIsolated::NoSessionDir)?;
    root::check(Path::new(root)).map_err(NotIsolated::NotASessionRoot)?;
    let address = environment
        .bus
        .as_ref()
        .and_then(|address| address.to_str())
        .ok_or(NotIsolated::NoBus)?;
    if !bus::is_private(address) {
        return Err(NotIsolated::NotAPrivateBus {
            address: address.to_owned(),
        });
    }
    Ok(Isolated {
        compositor: compositor.to_owned(),
        bus: address.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::made;

    /// What a client of `cargo xtask session kwin` has, with `root` its
    /// session's root.
    fn inside(root: &Path) -> Environment {
        Environment {
            compositor: Some(crate::KWIN.into()),
            session_dir: Some(root.as_os_str().to_owned()),
            bus: Some(
                "unix:abstract=/tmp/dbus-deskrinch-4103809-0,guid=9a46e0742c0824b06eb9c2446b03c0af"
                    .into(),
            ),
        }
    }

    #[test]
    fn inside_a_session_it_says_which_compositor_and_bus() {
        let root = made(true);
        assert_eq!(
            check(&inside(root.path())),
            Ok(Isolated {
                compositor: crate::KWIN.into(),
                bus: "unix:abstract=/tmp/dbus-deskrinch-4103809-0,guid=9a46e0742c0824b06eb9c2446b03c0af"
                    .into(),
            })
        );
    }

    #[test]
    fn each_condition_missing_is_named() {
        let root = made(true);
        let without = |change: fn(&mut Environment)| {
            let mut environment = inside(root.path());
            change(&mut environment);
            check(&environment).unwrap_err()
        };
        assert_eq!(without(|e| e.compositor = None), NotIsolated::NoCompositor);
        assert_eq!(
            without(|e| e.compositor = Some("".into())),
            NotIsolated::NoCompositor
        );
        assert_eq!(without(|e| e.session_dir = None), NotIsolated::NoSessionDir);
        assert_eq!(without(|e| e.bus = None), NotIsolated::NoBus);
        assert!(matches!(
            without(|e| e.session_dir = Some("/tmp".into())),
            NotIsolated::NotASessionRoot(_)
        ));
    }

    #[test]
    fn an_unmarked_root_is_not_a_session_s() {
        let unmarked = made(false);
        assert!(matches!(
            check(&inside(unmarked.path())),
            Err(NotIsolated::NotASessionRoot(NotARoot {
                why: "no session marker in it",
                ..
            }))
        ));
    }

    #[test]
    fn the_real_session_bus_is_refused() {
        let root = made(true);
        let real = Environment {
            bus: Some("unix:path=/run/user/1000/bus".into()),
            ..inside(root.path())
        };
        let refusal = check(&real).unwrap_err();
        assert_eq!(
            refusal,
            NotIsolated::NotAPrivateBus {
                address: "unix:path=/run/user/1000/bus".into()
            }
        );
        assert!(
            refusal
                .to_string()
                .contains("DBUS_SESSION_BUS_ADDRESS is unix:path=/run/user/1000/bus, not a session's private bus"),
            "{refusal}"
        );
    }

    #[test]
    fn a_refusal_says_how_to_run_it() {
        let refusal = check(&Environment::default()).unwrap_err().to_string();
        assert!(
            refusal.starts_with("not inside an isolated session"),
            "{refusal}"
        );
        assert!(
            refusal.contains("DESKRINCH_TEST_COMPOSITOR is not set, or empty, or not text"),
            "{refusal}"
        );
    }
}
