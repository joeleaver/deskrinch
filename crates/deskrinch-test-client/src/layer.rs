//! [`TestLayer`]: a small `wlr-layer-shell` surface on the overlay layer
//! that takes the seat's keyboard exclusively, for a test that takes the
//! keyboard from a client and gives it back
//! ([`TestClient::open_overlay`](crate::TestClient::open_overlay)).
//!
//! It is one of the client's surfaces like its windows: the keyboard's
//! focus reaches it through the same map of surfaces
//! ([`crate::keyboard`]), and it draws, waits and notes the keyboard as a
//! [`TestWindow`](crate::TestWindow) does.
//!
//! # What each compositor does
//!
//! Probed with the launcher (itself an overlay surface with the keyboard
//! exclusively) shown, and a second client's layer surface mapped after
//! it: did the second take the keyboard?
//!
//! | The second surface | KWin 6.6.6 | sway 1.11 | labwc 0.9.3 |
//! |---|---|---|---|
//! | overlay, keyboard exclusive | takes it | takes it | takes it |
//! | top, keyboard exclusive | takes it | takes it | does not |
//! | overlay, keyboard on demand, clicked | takes it | takes it | does not |
//! | top, keyboard on demand, clicked | takes it | takes it | does not |
//!
//! sway and labwc on wlroots 0.19.2. labwc keeps the keyboard on the first
//! overlay exclusive surface against the other three kinds; an overlay
//! exclusive surface takes it under all three, which is why this is one.
//! The clicked rows are not settled: under KWin and sway the second
//! surface may have taken the keyboard when it was mapped rather than when
//! it was clicked. When the surface goes, the keyboard leaves it.
//!
//! Under sway and labwc the keyboard can enter it from its first configure,
//! before its first buffer, and it is configured several times more:
//! [`TestLayer::wait_keyboard`] is what to wait on.
//!
//! Its own test is in `deskrinch-test-input`'s compositor suite, beside the
//! windows' keyboard tests: on a headless wlroots compositor the seat has a
//! keyboard only once that crate has made a virtual one, and it depends on
//! this crate, not the other way round.

use std::sync::Arc;

use deskrinch_test_wayland::Rgba;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    Layer, ZwlrLayerShellV1,
};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    self, KeyboardInteractivity, ZwlrLayerSurfaceV1,
};

use crate::client::Client;
use crate::seat::Windows;
use crate::window::Shared;
use crate::{Error, Paint, lock};

/// Its size, in surface coordinates: small, and in the middle of whichever
/// output the compositor puts it on.
const SIZE: (u32, u32) = (64, 64);

/// What it is called in errors, and its layer namespace.
const NAME: &str = "deskrinch-test-overlay";

/// What it draws: opaque, so that it is plainly mapped.
const PAINT: Paint = Paint::Solid(Rgba::opaque(0x20, 0x80, 0xe0));

/// A mapped overlay surface with the keyboard exclusively; dropping it
/// destroys it, and the keyboard leaves it.
#[derive(Debug)]
pub struct TestLayer {
    connection: Connection,
    windows: Windows,
    layer: ZwlrLayerSurfaceV1,
    pub(crate) shared: Arc<Shared>,
}

impl TestLayer {
    /// Makes `surface` an overlay surface through `shell` and commits it;
    /// the client's thread draws it at its first configure.
    pub(crate) fn open(
        connection: Connection,
        windows: Windows,
        shell: &ZwlrLayerShellV1,
        surface: &WlSurface,
        qh: &QueueHandle<Client>,
    ) -> Result<Self, Error> {
        let shared = Arc::new(Shared::new(NAME, surface.clone(), PAINT));
        lock(&windows).insert(surface.id(), Arc::clone(&shared));
        let layer = shell.get_layer_surface(
            surface,
            None,
            Layer::Overlay,
            NAME.to_owned(),
            qh,
            Arc::clone(&shared),
        );
        layer.set_size(SIZE.0, SIZE.1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        surface.commit();
        connection.flush().map_err(Error::Send)?;
        Ok(Self {
            connection,
            windows,
            layer,
            shared,
        })
    }

    /// Whether it has the keyboard focus.
    #[must_use]
    pub fn has_keyboard(&self) -> bool {
        self.shared.has_keyboard()
    }

    /// Waits until it has the keyboard focus.
    ///
    /// # Errors
    ///
    /// When it does not get it within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    pub fn wait_keyboard(&self) -> Result<(), Error> {
        self.shared.wait_keyboard()
    }
}

impl Drop for TestLayer {
    fn drop(&mut self) {
        lock(&self.windows).remove(&self.shared.surface.id());
        self.layer.destroy();
        self.shared.surface.destroy();
        drop(self.connection.flush());
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, Arc<Shared>> for Client {
    fn event(
        client: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        shared: &Arc<Shared>,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                layer.ack_configure(serial);
                // Zero is the client's choice: the size it asked for.
                let pick = |given: u32, asked: u32| if given == 0 { asked } else { given };
                let size = (pick(width, SIZE.0), pick(height, SIZE.1));
                let size = (
                    i32::try_from(size.0).unwrap_or(i32::MAX),
                    i32::try_from(size.1).unwrap_or(i32::MAX),
                );
                let mut status = lock(&shared.status);
                if status.drawn != Some(size) {
                    shared.draw(&mut status, client, size, qh);
                }
                shared.surface.commit();
                shared.changed.notify_all();
            }
            // The compositor took it away (its output went): it is
            // destroyed, as the protocol asks; dropping the `TestLayer`
            // later destroys the rest, and a request to the destroyed layer
            // surface is ignored.
            zwlr_layer_surface_v1::Event::Closed => {
                layer.destroy();
                shared.changed.notify_all();
            }
            _ => {}
        }
    }
}
