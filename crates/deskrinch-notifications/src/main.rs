//! `deskrinch-notifications`: the notification daemon. It is the session's
//! `org.freedesktop.Notifications`, and shows each live notification as a
//! popup in a column at the top right of the screen.
//!
//! # The service
//!
//! Everything a notification is and does is `deskrinch-notification-service`'s:
//! the bus interface, the model of the live notifications, expiry,
//! replacement, closing and its reasons, actions, and what an untrusted
//! sender may hand over. The daemon starts it with its choices ([`policy`]),
//! draws its model, and turns the user's acts into its operations.
//!
//! - **The name.** The daemon takes `org.freedesktop.Notifications` only
//!   when no one owns it, keeps it, and does not wait in line. Where another
//!   program owns it, the daemon says which and ends with deskrinch-shell's
//!   `CANNOT_RUN_HERE`, so it is not started again. Its starter waits for it
//!   to own the name: that is its being ready.
//! - **The capabilities** it reports are what the popups do: actions, and
//!   action icons ([`policy::CAPABILITIES`]). Links are plain text.
//! - **Expiry** is the sender's timeout, or five seconds when it leaves it
//!   to the server; a critical notification stays until closed. A popup
//!   under the pointer waits.
//! - **Closing.** The popup's close button dismisses it; the sender closes
//!   it; it expires; and when the daemon stops, every live notification is
//!   closed with the "undefined" reason, so no sender is left waiting.
//!
//! # The popups
//!
//! One layer surface, at the top right, holds the column ([`surface`]); it
//! exists only while a notification shows. The look is the shell's
//! (`deskrinch-theme`), following the desktop's colour scheme. A popup's
//! icon is the first icon name of its image (`image-path`) and then its
//! `app_icon`, else its `desktop-entry` application's icon, from the icon
//! theme ([`picture`]); pixels and files a sender names are not drawn yet.
//!
//! # How it ends
//!
//! - Its display going away: deskrinch-shell's `DISPLAY_LOST`
//!   (`deskrinch_component::app::run`).
//! - Its bus connection ending, or the name taken from it: a failure, so it
//!   is started again.
//! - The name owned by another program at start: `CANNOT_RUN_HERE`.
//!
//! # Logging and rendering
//!
//! As every component: `tracing` set up first, `RUST_LOG` choosing what is
//! shown (`warn` and worse by default); rinch's GPU renderer, or its
//! software one, chosen at run time, `RINCH_RENDERER` overriding.

mod actions;
mod feed;
mod owner;
mod picture;
mod policy;
mod popup;
mod root;
mod style;
mod surface;

use std::process::ExitCode;
use std::sync::Arc;

use deskrinch_appearance::Bus;
use deskrinch_applications::{Icons, Installed};
use deskrinch_component::{Configuration, report};
use deskrinch_desktop_entries::Environment;
use deskrinch_icons::SearchPath;
use deskrinch_notification_service::{NAME, Service, StartError, SystemClock};
use deskrinch_shell::exit_status::CANNOT_RUN_HERE;
use deskrinch_theme::Theme;
use deskrinch_xdg::BaseDirs;

use crate::feed::Feed;

fn main() -> ExitCode {
    // Before anything is reported.
    report::init();
    let feed = Arc::new(Feed::default());
    let subscriber = Arc::clone(&feed);
    let config = policy::config();
    let bus = config.bus.clone();
    let service = match Service::start(config, Arc::new(SystemClock), move |event| {
        subscriber.event(&event);
    }) {
        Ok(service) => service,
        Err(StartError::NameTaken) => {
            let owner = owner::describe(&bus, NAME).unwrap_or_else(|| "another program".to_owned());
            tracing::error!(
                "{NAME} is owned by {owner}, which shows the notifications; the notification \
                 daemon cannot run here, and ends"
            );
            return ExitCode::from(CANNOT_RUN_HERE);
        }
        Err(error) => {
            report::error("starting the notification service", &error);
            return ExitCode::FAILURE;
        }
    };
    let handle = service.handle();
    feed.started(handle.clone());
    let base_dirs = BaseDirs::from_process();
    let configuration = Configuration::start(&base_dirs);
    let theme = Theme::start(&configuration, Bus::Session);
    let first_theme = theme.initial();
    let parts = root::Parts {
        feed: Arc::clone(&feed),
        handle,
        theme,
        installed: Installed::start(&Environment::from_process()),
        icons: Icons::start(SearchPath::new(&base_dirs)),
    };
    let ended = deskrinch_component::app::run(
        deskrinch_component::app::windowless(move |scope| root::notifications(scope, parts))
            .theme(first_theme),
    );
    // An orderly stop: every live notification closed (reason 4), then the
    // name released.
    drop(service);
    if feed.ended() {
        ExitCode::FAILURE
    } else {
        ended
    }
}
