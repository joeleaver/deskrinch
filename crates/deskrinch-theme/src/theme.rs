//! A component's theme while it runs: the setting and the portal, resolved
//! and applied.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use deskrinch_appearance::{Appearance, Bus, ColorScheme, Event};
use deskrinch_component::{Configuration, Handover, LiveSettings, report};
use deskrinch_settings::general::GeneralSettings;
use rinch::prelude::{Memo, ThemeProviderProps};
use rinch::reactive::Effect;

use crate::look;
use crate::scheme::{Scheme, resolve};

/// How long [`Theme::start`] waits for the portal's first answer, so that
/// the window is made in the right scheme rather than switched a moment
/// later. A portal that takes longer (one the bus is still starting) is
/// applied when it answers.
const FIRST_ANSWER: Duration = Duration::from_secs(1);

/// The shell's theme for one component: the `[general]` `color-scheme`
/// setting and the desktop's colour scheme from the settings portal, both
/// followed, resolved to dark or light ([`resolve`]) and applied to the
/// component's window.
///
/// [`start`](Self::start) it before the window exists and make the window
/// with [`initial`](Self::initial); in the root component,
/// [`apply`](Self::apply) keeps the window's theme following both.
pub struct Theme {
    general: LiveSettings<GeneralSettings>,
    /// The portal's answers (`None`: no portal), for the UI.
    portal: Arc<Handover<Option<ColorScheme>>>,
    /// The portal's answer when the component started.
    first_portal: Option<ColorScheme>,
    /// Kept while the component runs; `None` when the bus could not be
    /// reached.
    appearance: Option<Appearance>,
}

impl Theme {
    /// Follows `[general]` from the component's `configuration`, and the
    /// settings portal on `bus` (usually [`Bus::Session`]); waits a moment
    /// (at most a second) for the portal's first answer. When the bus cannot
    /// be reached (which is reported), the theme follows the setting alone.
    #[must_use]
    pub fn start(configuration: &Configuration, bus: Bus) -> Self {
        let general = configuration.follow::<GeneralSettings>();
        let portal = Arc::new(Handover::default());
        let (first, first_answer) = mpsc::channel();
        let deliver = Arc::clone(&portal);
        let appearance = Appearance::start(bus, move |event| {
            let answer = match event {
                Event::ColorScheme(scheme) => Some(scheme),
                Event::NoPortal => None,
                Event::Error(error) => {
                    report::error("following the desktop's colour scheme", &error);
                    return;
                }
            };
            deliver.deliver(answer);
            // Heard only until `start` returns; later answers need no one.
            first.send(answer).ok();
        });
        let appearance = match appearance {
            Ok(appearance) => Some(appearance),
            Err(error) => {
                report::error("following the desktop's colour scheme", &error);
                None
            }
        };
        let first_portal = match appearance {
            Some(_) => first_answer.recv_timeout(FIRST_ANSWER).ok().flatten(),
            None => None,
        };
        Self {
            general,
            portal,
            first_portal,
            appearance,
        }
    }

    /// The scheme when the component started.
    #[must_use]
    pub fn initial_scheme(&self) -> Scheme {
        resolve(self.general.initial.color_scheme, self.first_portal)
    }

    /// The theme properties to make the window with (`App::theme`).
    #[must_use]
    pub fn initial(&self) -> ThemeProviderProps {
        look::props(self.initial_scheme())
    }

    /// Keeps the window's theme following the setting and the portal, in
    /// the root component, on the UI thread: the resolved scheme, as a
    /// memo, and the window restyled (`rinch::update_theme`) whenever it
    /// changes. Both are followed until the component is disposed.
    #[must_use = "the scheme, for a widget that shows it; the theme is applied either way"]
    pub fn apply(self) -> Memo<Scheme> {
        let mut applied = self.initial_scheme();
        let general = self.general.into_signal();
        let portal = self.portal.signal(self.first_portal);
        let scheme = Memo::new(move || resolve(general.get().color_scheme, portal.get()));
        Effect::new(move || {
            let wanted = scheme.get();
            if wanted != applied {
                rinch::update_theme(&look::props(wanted));
                applied = wanted;
            }
        });
        let appearance = self.appearance;
        rinch::reactive::on_cleanup(move || drop(appearance));
        scheme
    }
}

impl fmt::Debug for Theme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Theme")
            .field("setting", &self.general.initial.color_scheme)
            .field("portal", &self.first_portal)
            .field("following_portal", &self.appearance.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskrinch_test_dbus::PrivateBus;
    use deskrinch_xdg::BaseDirs;
    use std::ffi::OsString;

    fn configuration(root: &std::path::Path, text: &str) -> Configuration {
        let dir = root.join("home/deskrinch");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), text).unwrap();
        let home: OsString = root.join("home").into();
        let system: OsString = root.join("etc").into();
        Configuration::start(&BaseDirs::from_vars(|name| match name {
            "XDG_CONFIG_HOME" => Some(home.clone()),
            "XDG_CONFIG_DIRS" => Some(system.clone()),
            _ => None,
        }))
    }

    #[test]
    fn with_no_portal_it_is_the_settings_scheme_or_dark() {
        let bus = PrivateBus::start().expect("the theme tests need dbus-daemon");
        let root = tempfile::tempdir().unwrap();
        let system = Theme::start(
            &configuration(root.path(), ""),
            Bus::Address(bus.address().to_owned()),
        );
        assert_eq!(system.initial_scheme(), Scheme::Dark);
        assert!(system.initial().dark_mode);
        let light = Theme::start(
            &configuration(root.path(), "[general]\ncolor-scheme = \"light\"\n"),
            Bus::Address(bus.address().to_owned()),
        );
        assert_eq!(light.initial_scheme(), Scheme::Light);
        assert!(!light.initial().dark_mode);
    }

    #[test]
    fn with_no_bus_it_follows_the_setting_alone() {
        let root = tempfile::tempdir().unwrap();
        let theme = Theme::start(
            &configuration(root.path(), "[general]\ncolor-scheme = \"light\"\n"),
            Bus::Address("unix:path=/nonexistent/bus".into()),
        );
        assert_eq!(theme.initial_scheme(), Scheme::Light);
        assert!(format!("{theme:?}").contains("following_portal: false"));
    }
}
