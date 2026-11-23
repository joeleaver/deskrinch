//! [`TestLayer`]: a `wlr-layer-shell` surface. Either a small one on the
//! overlay layer that takes the seat's keyboard exclusively, for a test
//! that takes the keyboard from a client and gives it back
//! ([`TestClient::open_overlay`](crate::TestClient::open_overlay)); or one
//! with the namespace, layer and size a test chooses, without the keyboard,
//! for a test of what a compositor makes of a namespace
//! ([`TestClient::open_layer`](crate::TestClient::open_layer)).
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
//! Dropping it while the client's thread handles a configure is safe, by
//! the order of the destroys: the layer surface first, then the surface.
//! The configure is acknowledged through the layer surface itself, so an
//! acknowledgement sent after its destroy is dropped on the client's side,
//! and a buffer committed then goes to a surface with no role object,
//! which a compositor takes as unmapped. (A window is not so: it is
//! acknowledged through its `xdg_surface`, which outlives its toplevel by
//! one request, and is ordered by a lock instead, [`crate::TestWindow`].)
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

/// The overlay's size, in surface coordinates: small, and in the middle of
/// whichever output the compositor puts it on.
const SIZE: (u32, u32) = (64, 64);

/// The overlay's layer namespace, and what it is called in errors.
const NAME: &str = "deskrinch-test-overlay";

/// A layer of `wlr-layer-shell`, bottom to top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLayer {
    /// `background`.
    Background,
    /// `bottom`.
    Bottom,
    /// `top`.
    Top,
    /// `overlay`.
    Overlay,
}

impl ShellLayer {
    fn protocol(self) -> Layer {
        match self {
            Self::Background => Layer::Background,
            Self::Bottom => Layer::Bottom,
            Self::Top => Layer::Top,
            Self::Overlay => Layer::Overlay,
        }
    }
}

/// What a layer surface is made as.
#[derive(Debug, Clone)]
pub(crate) struct LayerSpec {
    /// Its namespace, which also names it in errors.
    pub(crate) namespace: String,
    /// Its layer.
    pub(crate) layer: ShellLayer,
    /// Its size, in surface coordinates.
    pub(crate) size: (u32, u32),
    /// Whether it takes the keyboard exclusively.
    pub(crate) keyboard: bool,
}

impl LayerSpec {
    /// The overlay [`TestClient::open_overlay`](crate::TestClient::open_overlay) opens.
    pub(crate) fn overlay() -> Self {
        Self {
            namespace: NAME.to_owned(),
            layer: ShellLayer::Overlay,
            size: SIZE,
            keyboard: true,
        }
    }
}

/// What the layer surface's events are told: its window's state and the
/// size it asked for.
#[derive(Debug)]
pub(crate) struct LayerData {
    shared: Arc<Shared>,
    size: (u32, u32),
}

/// What it draws: opaque, so that it is plainly mapped.
const PAINT: Paint = Paint::Solid(Rgba::opaque(0x20, 0x80, 0xe0));

/// A mapped layer surface; dropping it destroys it, and the keyboard
/// leaves it if it had it.
#[derive(Debug)]
pub struct TestLayer {
    connection: Connection,
    windows: Windows,
    layer: ZwlrLayerSurfaceV1,
    pub(crate) shared: Arc<Shared>,
}

impl TestLayer {
    /// Makes `surface` a layer surface as `spec` says through `shell` and
    /// commits it; the client's thread draws it at its first configure.
    pub(crate) fn open(
        connection: Connection,
        windows: Windows,
        shell: &ZwlrLayerShellV1,
        surface: &WlSurface,
        spec: &LayerSpec,
        qh: &QueueHandle<Client>,
    ) -> Result<Self, Error> {
        let shared = Arc::new(Shared::new(&spec.namespace, surface.clone(), PAINT));
        lock(&windows).insert(surface.id(), Arc::clone(&shared));
        let layer = shell.get_layer_surface(
            surface,
            None,
            spec.layer.protocol(),
            spec.namespace.clone(),
            qh,
            LayerData {
                shared: Arc::clone(&shared),
                size: spec.size,
            },
        );
        layer.set_size(spec.size.0, spec.size.1);
        layer.set_keyboard_interactivity(if spec.keyboard {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        });
        surface.commit();
        connection.flush().map_err(Error::Send)?;
        Ok(Self {
            connection,
            windows,
            layer,
            shared,
        })
    }

    /// The size of the buffer it last drew, in surface coordinates.
    #[must_use]
    pub fn drawn(&self) -> Option<(i32, i32)> {
        lock(&self.shared.status).drawn
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

impl Dispatch<ZwlrLayerSurfaceV1, LayerData> for Client {
    fn event(
        client: &mut Self,
        layer: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        data: &LayerData,
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
                let shared = &data.shared;
                let size = (pick(width, data.size.0), pick(height, data.size.1));
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
                data.shared.changed.notify_all();
            }
            _ => {}
        }
    }
}
