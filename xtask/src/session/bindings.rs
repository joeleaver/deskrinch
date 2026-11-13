//! A session's key bindings in force: the session is ready only once its
//! compositor would act on each.
//!
//! sway and labwc read their bindings from the configuration they start
//! with. KWin's are its global shortcuts, which kglobalacceld, inside KWin,
//! registers after KWin has started: it reads the applications in KDE's
//! service cache, which it may first have to build, in a fresh home. A chord
//! typed before then is lost. So under KWin the session waits until
//! kglobalacceld has registered each binding's application as a component
//! with its `_launch` action.

use std::thread;
use std::time::{Duration, Instant};

use deskrinch_bus::Bus;

use super::display::Binding;
use super::{Compositor, POLL, Session};
use crate::error::Error;

/// How long kglobalacceld has to register the bindings: it may build KDE's
/// service cache first.
const REGISTERED: Duration = Duration::from_secs(60);

impl Session {
    /// Waits until the compositor has each of `bindings` in force.
    pub(super) fn wait_for_bindings(&self, bindings: &[Binding]) -> Result<(), Error> {
        if self.compositor != Compositor::Kwin {
            return Ok(());
        }
        for binding in bindings {
            let path = component_path(&binding.id);
            let deadline = Instant::now() + REGISTERED;
            while !launch_registered(self.bus.address(), &path) {
                super::interrupt::check()?;
                if Instant::now() >= deadline {
                    return Err(Error::BindingNotInForce {
                        compositor: self.compositor.name(),
                        binding: binding.id.clone(),
                        log: self.log(),
                    });
                }
                thread::sleep(POLL);
            }
        }
        Ok(())
    }
}

/// The object kglobalacceld serves the component of the application
/// `<id>.desktop` at: its name with every character other than an ASCII
/// letter, digit or `_` made `_`, under `/component/`.
fn component_path(id: &str) -> String {
    let name: String = format!("{id}.desktop")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("/component/{name}")
}

/// Whether kglobalacceld, on the bus at `address`, serves the component at
/// `path` with its `_launch` action; not yet when it cannot be asked.
fn launch_registered(address: &str, path: &str) -> bool {
    let names: zbus::Result<Vec<String>> = zbus::block_on(async {
        let connection = Bus::Address(address.to_owned())
            .builder()
            .method_timeout(Duration::from_secs(5))
            .build()
            .await?;
        let reply = connection
            .call_method(
                Some("org.kde.kglobalaccel"),
                path,
                Some("org.kde.kglobalaccel.Component"),
                "shortcutNames",
                &(),
            )
            .await?;
        reply.body().deserialize()
    });
    names.is_ok_and(|names| names.iter().any(|name| name == "_launch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_component_as_kglobalacceld_does() {
        // As KWin 6.6 served it.
        assert_eq!(
            component_path("deskrinch-launcher-toggle"),
            "/component/deskrinch_launcher_toggle_desktop"
        );
    }
}
