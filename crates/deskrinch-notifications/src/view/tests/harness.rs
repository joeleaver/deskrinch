//! The column's content in rinch's test harness, fed by a real service:
//! the service on a private bus of the test's own
//! (`deskrinch-test-dbus`), a client sending to it, and the content
//! mounted in a window of the column's props at the height the test
//! chooses. After each change the test thread hands the service's list to
//! the content, as the daemon's feed does, and the harness settles: the
//! layout, the popups' reports, the fit and the layout it causes.
//!
//! Nothing here is sent across threads to the UI: the harness's
//! main-thread queue is the process's, shared by tests running at once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use deskrinch_applications::Applications;
use deskrinch_notification_service::{
    Bus, Config, Expiry, Handle, ManualClock, NotificationId, Service,
};
use deskrinch_test_notify::Client;
use deskrinch_theme::{Scheme, look};
use rinch::WindowProps;
use rinch::prelude::*;
use rinch::testing::TestApp;
use zbus::Variant;

use crate::column::Column;
use crate::feed::State;
use crate::policy::WIDTH;
use crate::popup::Shared;
use crate::surface;
use crate::view;

/// A timeout every test notification has: long, and on a clock that does
/// not move, so that a pause shows as one and nothing expires.
const TIMEOUT: i32 = 60_000;

/// One popup as the content shows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Shown {
    pub(super) summary: String,
    pub(super) waiting: bool,
    pub(super) critical: bool,
    /// Taken out of the input region (`data-input-region="none"`).
    pub(super) outside_input: bool,
    pub(super) top: f32,
    pub(super) bottom: f32,
}

/// The service, its client, and the content mounted.
pub(super) struct Harness {
    pub(super) app: TestApp,
    state: Signal<State>,
    client: Client,
    handle: Handle,
    _service: Service,
    _bus: deskrinch_test_dbus::PrivateBus,
}

impl Harness {
    /// The content mounted `height` logical pixels high, nothing live yet.
    pub(super) fn at(height: u32) -> Self {
        let bus =
            deskrinch_test_dbus::PrivateBus::start().expect("a private bus (needs dbus-daemon)");
        let config = Config {
            bus: Bus::Address(bus.address().to_owned()),
            ..Config::default()
        };
        let service = Service::start(config, Arc::new(ManualClock::new()), |_| {}).unwrap();
        let handle = service.handle();
        let connection =
            zbus::block_on(zbus::connection::Builder::address(bus.address()).build()).unwrap();
        let client = Client::on(&connection);
        let slot: Rc<RefCell<Option<Signal<State>>>> = Rc::default();
        let (made, mounted) = (Rc::clone(&slot), handle.clone());
        let app = TestApp::builder(move |__scope: &mut RenderScope| {
            let state = Signal::new(State::default());
            *made.borrow_mut() = Some(state);
            let order = view::order(state);
            let shared = Shared {
                handle: mounted.clone(),
                state,
                index: Signal::new(None),
                applications: Signal::new(Arc::new(Applications::default())),
                column: Column::new(mounted, order),
            };
            view::content(__scope, order, &shared)
        })
        .window_props(WindowProps {
            width: WIDTH,
            height,
            transparent: true,
            wayland_layer: Some(surface::layer_surface()),
            ..WindowProps::default()
        })
        .theme(look::props(Scheme::Dark))
        .mount();
        let state = slot.borrow().expect("made at the mount");
        Self {
            app,
            state,
            client,
            handle,
            _service: service,
            _bus: bus,
        }
    }

    /// Sends a notification, `critical` or normal, and shows the column
    /// the service's list; its id.
    pub(super) fn send(&mut self, summary: &str, body: &str, critical: bool) -> NotificationId {
        let hints: HashMap<&str, Variant<'_>> = if critical {
            HashMap::from([("urgency", Variant::from(2_u8))])
        } else {
            HashMap::new()
        };
        let id = self
            .client
            .send(0, summary, body, &[], &hints, TIMEOUT)
            .unwrap();
        self.follow();
        NotificationId::new(id).unwrap()
    }

    /// The sender closes `id`, and the column is shown the service's list.
    pub(super) fn close(&mut self, id: NotificationId) {
        self.client.close(id.get()).unwrap();
        self.follow();
    }

    /// The window `height` high now, as the compositor's configure makes
    /// it.
    pub(super) fn resize(&mut self, height: u32) {
        self.app.resize(WIDTH, height);
        self.app.settle();
    }

    /// The service's list handed to the content, which settles.
    fn follow(&mut self) {
        self.state.set(State {
            live: Arc::new(self.handle.notifications()),
            ended: false,
        });
        self.app.settle();
    }

    /// The popups, top to bottom in the column's order.
    pub(super) fn popups(&self) -> Vec<Shown> {
        let summaries = self.app.query_all(".summary");
        self.app
            .query_all(".popup")
            .into_iter()
            .zip(summaries)
            .map(|(popup, summary)| {
                let class = self.app.attribute(&popup, "class").unwrap_or_default();
                let classes: Vec<&str> = class.split_whitespace().collect();
                let bounds = self.app.bounds(&popup);
                Shown {
                    summary: self.app.text(&summary),
                    waiting: classes.contains(&"waiting"),
                    critical: classes.contains(&"critical"),
                    outside_input: self.app.attribute(&popup, "data-input-region").as_deref()
                        == Some("none"),
                    top: bounds.y,
                    bottom: bounds.y + bounds.height,
                }
            })
            .collect()
    }

    /// The summaries shown and those waiting, each in the column's order.
    pub(super) fn fit(&self) -> (Vec<String>, Vec<String>) {
        let (waiting, shown): (Vec<Shown>, Vec<Shown>) =
            self.popups().into_iter().partition(|p| p.waiting);
        let names = |popups: Vec<Shown>| popups.into_iter().map(|p| p.summary).collect();
        (names(shown), names(waiting))
    }

    /// The count line's text, while it shows.
    pub(super) fn count_line(&self) -> Option<String> {
        self.app.query(".count").map(|count| self.app.text(&count))
    }

    /// The window height, in whole logical pixels, at which the room's
    /// bottom is `spare` pixels or a little more under `bottom` (a place in
    /// the window), the column as it is now (with its count line or not).
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window's height: positive, and hundreds of pixels"
    )]
    pub(super) fn height_for(&self, bottom: f32, spare: u32) -> u32 {
        let column = self.app.bounds(".column");
        let below = column.y + column.height - self.room_bottom();
        (bottom + below).ceil() as u32 + spare
    }

    /// The popups' container's top edge: the room's.
    pub(super) fn room_top(&self) -> f32 {
        self.app.bounds(".popups").y
    }

    /// The popups' container's bottom edge: the room's.
    pub(super) fn room_bottom(&self) -> f32 {
        let room = self.app.bounds(".popups");
        room.y + room.height
    }

    /// Whether the service holds `id`'s expiry.
    pub(super) fn paused(&self, id: NotificationId) -> bool {
        self.handle
            .notification(id)
            .is_some_and(|n| matches!(n.expiry, Expiry::Paused { .. }))
    }

    /// Asserts what the column shows is consistent: every shown popup whole
    /// in the room, every waiting one hidden and out of the input region,
    /// the count line there exactly while some wait and saying how many.
    pub(super) fn assert_consistent(&self) {
        let room = self.room_bottom();
        let popups = self.popups();
        for popup in &popups {
            if popup.waiting {
                assert!(
                    popup.outside_input,
                    "{popup:?} waits inside the input region"
                );
            } else {
                assert!(
                    popup.bottom <= room,
                    "{popup:?} is cut by the room at {room}"
                );
                assert!(
                    !popup.outside_input,
                    "{popup:?} is shown outside the input region"
                );
            }
        }
        let waiting = popups.iter().filter(|p| p.waiting).count();
        let count = (waiting > 0).then(|| format!("{waiting} more"));
        assert_eq!(self.count_line(), count, "the count line, for {popups:?}");
    }
}
