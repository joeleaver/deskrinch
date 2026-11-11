//! A component's theme while it runs: the setting and the portal, resolved
//! and applied.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use deskrinch_appearance::{Appearance, Bus, Event, IconThemes, Preferences};
use deskrinch_component::{Configuration, Handover, LiveSettings, report};
use deskrinch_settings::general::{ColorSchemeSetting, GeneralSettings, IconThemeSetting};
use rinch::prelude::{Memo, ThemeProviderProps};
use rinch::reactive::Effect;

use crate::look;
use crate::scheme::{Scheme, resolve};

/// How long [`Theme::start`] waits for the portal's first answer when the
/// setting is `"system"`, so that the window is made in the right scheme
/// rather than opened dark and switched a moment later. A portal that takes
/// longer (one the bus is still starting) is applied when it answers. With
/// `"dark"` or `"light"` the answer cannot change the first frame, so
/// there is no wait.
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
    portal: Arc<Handover<Option<Preferences>>>,
    /// The portal's answer when the component started.
    first_portal: Option<Preferences>,
    /// Kept while the component runs; `None` when the bus could not be
    /// reached.
    appearance: Option<Appearance>,
}

impl Theme {
    /// Follows `[general]` from the component's `configuration`, and the
    /// settings portal on `bus` (usually [`Bus::Session`]). When the setting
    /// is `"system"` it waits a moment (at most a second) for the portal's
    /// first answer; with `"dark"` or `"light"` it does not wait, and still
    /// follows the portal for when the setting changes. When the bus cannot
    /// be reached (which is reported), the theme follows the setting alone.
    #[must_use]
    pub fn start(configuration: &Configuration, bus: Bus) -> Self {
        let general = configuration.follow::<GeneralSettings>();
        let portal = Arc::new(Handover::default());
        let (first, first_answer) = mpsc::channel();
        let deliver = Arc::clone(&portal);
        let appearance = Appearance::start(bus, move |event| {
            let answer = match event {
                Event::Preferences(preferences) => Some(preferences),
                Event::NoPortal => None,
                Event::Error(error) => {
                    report::error("following the desktop's colour scheme", &error);
                    return;
                }
            };
            deliver.deliver(answer.clone());
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
        let decided = general.initial.color_scheme != ColorSchemeSetting::System;
        let first_portal = match appearance {
            Some(_) if !decided => first_answer.recv_timeout(FIRST_ANSWER).ok().flatten(),
            _ => None,
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
        resolve(
            self.general.initial.color_scheme,
            self.first_portal.as_ref().map(|p| p.color_scheme),
        )
    }

    /// The theme properties to make the window with (`App::theme`).
    #[must_use]
    pub fn initial(&self) -> ThemeProviderProps {
        look::props(self.initial_scheme())
    }

    /// Keeps the window's theme following the setting and the portal, in
    /// the root component, on the UI thread: the window restyled
    /// (`rinch::update_theme`) whenever the resolved scheme changes, and
    /// what was applied, as memos ([`Applied`]). Both are followed until the
    /// component is disposed.
    #[must_use = "the scheme and the icon theme's inputs, for widgets that use them; the theme is applied either way"]
    pub fn apply(self) -> Applied {
        let mut applied = self.initial_scheme();
        let general = self.general.into_signal();
        let portal = self.portal.signal(self.first_portal);
        let scheme = Memo::new(move || {
            let preferred = portal.with(|p| p.as_ref().map(|p| p.color_scheme));
            resolve(general.with(|g| g.color_scheme), preferred)
        });
        Effect::new(move || {
            let wanted = scheme.get();
            if wanted != applied {
                rinch::update_theme(&look::props(wanted));
                applied = wanted;
            }
        });
        let icon_theme = Memo::new(move || IconThemeChoice {
            setting: general.with(|g| g.icon_theme.clone()),
            portal: portal.with(|p| {
                p.as_ref()
                    .map(|p| p.icon_themes.clone())
                    .unwrap_or_default()
            }),
        });
        let appearance = self.appearance;
        rinch::reactive::on_cleanup(move || drop(appearance));
        Applied { scheme, icon_theme }
    }
}

/// What [`Theme::apply`] follows, for the component's own widgets.
#[derive(Debug, Clone, Copy)]
pub struct Applied {
    /// Dark or light, as applied to the window.
    pub scheme: Memo<Scheme>,
    /// What decides the icon theme: the `[general]` `icon-theme` setting
    /// and the portal's icon themes. A component that shows icons resolves
    /// it against what is installed ([`resolve_icon_theme`](crate::resolve_icon_theme)).
    pub icon_theme: Memo<IconThemeChoice>,
}

/// The inputs of the icon theme's resolution, as followed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IconThemeChoice {
    /// `[general]` `icon-theme`.
    pub setting: IconThemeSetting,
    /// The icon themes the settings portal states (none: no portal).
    pub portal: IconThemes,
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
    use deskrinch_appearance::ColorScheme;
    use deskrinch_test_dbus::PrivateBus;
    use deskrinch_test_portal::{COLOR_SCHEME, Methods, StandIn, Value};
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

    /// The stand-in portal on `bus`, answering `value` at once.
    fn portal(bus: &PrivateBus, value: u32) -> StandIn {
        let settings = [(COLOR_SCHEME, Value::Number(value))];
        StandIn::start(bus.address(), settings, Methods::Current).unwrap()
    }

    #[test]
    fn with_system_it_waits_for_the_portals_first_answer() {
        let bus = PrivateBus::start().expect("the theme tests need dbus-daemon");
        let _portal = portal(&bus, 2);
        let root = tempfile::tempdir().unwrap();
        let theme = Theme::start(
            &configuration(root.path(), ""),
            Bus::Address(bus.address().to_owned()),
        );
        assert_eq!(
            theme.first_portal.as_ref().map(|p| p.color_scheme),
            Some(ColorScheme::PreferLight)
        );
        assert_eq!(theme.initial_scheme(), Scheme::Light);
    }

    #[test]
    fn with_an_explicit_setting_it_does_not_wait_but_still_follows() {
        let bus = PrivateBus::start().expect("the theme tests need dbus-daemon");
        // Answers at once: had `start` waited, it would have its answer.
        let _portal = portal(&bus, 1);
        let root = tempfile::tempdir().unwrap();
        let theme = Theme::start(
            &configuration(root.path(), "[general]\ncolor-scheme = \"light\"\n"),
            Bus::Address(bus.address().to_owned()),
        );
        assert_eq!(
            theme.first_portal, None,
            "start did not wait for the portal"
        );
        assert_eq!(theme.initial_scheme(), Scheme::Light);
        assert!(theme.appearance.is_some(), "the portal is still followed");
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
