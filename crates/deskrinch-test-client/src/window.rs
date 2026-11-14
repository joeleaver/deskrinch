//! [`TestWindow`]: a mapped `xdg_toplevel` that acknowledges each configure
//! and draws its [`Paint`] at the size asked for. A close request is
//! recorded and obeyed: the toplevel is destroyed, as an application would
//! destroy it.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use deskrinch_test_wayland::{ShmBuffer, ShmError};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1;
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};

use crate::client::Client;
use crate::seat::Windows;
use crate::{Error, Paint, lock};
use deskrinch_test_wait::Deadline;

/// The size a window draws when the compositor leaves it the choice.
const DEFAULT_SIZE: (i32, i32) = (320, 240);

/// What the client's thread reports about one window.
#[derive(Debug, Default)]
pub(crate) struct Status {
    /// The size the last toplevel configure asked for (0 = client's choice),
    /// and whether it said the window is fullscreen.
    asked: (i32, i32),
    asked_fullscreen: bool,
    /// The size of the buffer attached, once there is one, and whether it
    /// was drawn for a fullscreen configure.
    drawn: Option<(i32, i32)>,
    drawn_fullscreen: bool,
    close_requested: bool,
    /// Why a buffer could not be made, if one could not.
    failure: Option<ShmError>,
    /// What it heard of the keyboard and the pointer.
    pub(crate) input: Heard,
}

/// What a window heard of the seat.
#[derive(Debug, Default)]
pub(crate) struct Heard {
    /// Whether it has the keyboard focus.
    pub(crate) keyboard: bool,
    /// The keys pressed while it had the focus, by evdev code, in order.
    pub(crate) keys: Vec<u32>,
    /// The pointer's events while it was over the window, in order.
    pub(crate) pointer: Vec<crate::PointerEvent>,
}

/// One window, shared by the test and the client's thread.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) title: String,
    pub(crate) surface: WlSurface,
    pub(crate) paint: Paint,
    pub(crate) status: Mutex<Status>,
    /// Notified whenever `status` changes.
    pub(crate) changed: Condvar,
}

/// A mapped toplevel; dropping it destroys it.
#[derive(Debug)]
pub struct TestWindow {
    connection: Connection,
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    windows: Windows,
    toplevel: XdgToplevel,
    /// Its `xdg-decoration` object, where the compositor offers one.
    decoration: Option<ZxdgToplevelDecorationV1>,
    xdg_surface: XdgSurface,
    pub(crate) shared: Arc<Shared>,
}

impl TestWindow {
    pub(crate) fn new(
        connection: Connection,
        outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
        windows: Windows,
        toplevel: XdgToplevel,
        decoration: Option<ZxdgToplevelDecorationV1>,
        xdg_surface: XdgSurface,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            connection,
            outputs,
            windows,
            toplevel,
            decoration,
            xdg_surface,
            shared,
        }
    }

    pub(crate) fn toplevel(&self) -> &XdgToplevel {
        &self.toplevel
    }

    /// Changes the title.
    ///
    /// # Errors
    ///
    /// When the request cannot be sent.
    pub fn set_title(&self, title: &str) -> Result<(), Error> {
        self.toplevel.set_title(title.to_owned());
        self.commit()
    }

    /// Asks to be maximized, or no longer.
    ///
    /// # Errors
    ///
    /// When the request cannot be sent.
    pub fn set_maximized(&self, maximized: bool) -> Result<(), Error> {
        if maximized {
            self.toplevel.set_maximized();
        } else {
            self.toplevel.unset_maximized();
        }
        self.commit()
    }

    /// Asks to be fullscreen (on the named output, or one the compositor
    /// picks for `None`), or no longer fullscreen.
    ///
    /// # Errors
    ///
    /// When there is no output so named, or the request cannot be sent.
    pub fn set_fullscreen(&self, fullscreen: bool, output: Option<&str>) -> Result<(), Error> {
        if fullscreen {
            let output = output
                .map(|name| {
                    lock(&self.outputs)
                        .get(name)
                        .cloned()
                        .ok_or_else(|| Error::UnknownOutput(name.to_owned()))
                })
                .transpose()?;
            self.toplevel.set_fullscreen(output.as_ref());
        } else {
            self.toplevel.unset_fullscreen();
        }
        self.commit()
    }

    /// Whether the compositor has asked this window to close.
    #[must_use]
    pub fn close_requested(&self) -> bool {
        lock(&self.shared.status).close_requested
    }

    /// The size of the buffer it last drew, in surface coordinates (its
    /// buffer scale is 1).
    #[must_use]
    pub fn drawn(&self) -> Option<(i32, i32)> {
        lock(&self.shared.status).drawn
    }

    /// Waits until it has drawn a buffer of a size `accept` takes, and
    /// returns that size; `what` names it in the error.
    ///
    /// # Errors
    ///
    /// When no such buffer is drawn within [`PATIENCE`](deskrinch_test_wait::PATIENCE), or a buffer could
    /// not be made.
    pub fn wait_drawn(
        &self,
        what: &str,
        accept: impl Fn((i32, i32)) -> bool,
    ) -> Result<(i32, i32), Error> {
        self.wait(what, |status| status.drawn.filter(|&size| accept(size)))
    }

    /// Waits until it has drawn a buffer for a configure that made it
    /// fullscreen, and returns that buffer's size: the output's logical
    /// size.
    ///
    /// # Errors
    ///
    /// As [`wait_drawn`](Self::wait_drawn).
    pub fn wait_fullscreen(&self) -> Result<(i32, i32), Error> {
        self.wait("a fullscreen buffer", |status| {
            status.drawn.filter(|_| status.drawn_fullscreen)
        })
    }

    /// Whether it has the keyboard focus.
    #[must_use]
    pub fn has_keyboard(&self) -> bool {
        lock(&self.shared.status).input.keyboard
    }

    /// Waits until it has the keyboard focus.
    ///
    /// # Errors
    ///
    /// When it does not get it within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    pub fn wait_keyboard(&self) -> Result<(), Error> {
        self.wait_input("the keyboard focus", |status| status.input.keyboard)
    }

    /// Waits until the key with evdev code `key` has been pressed while it
    /// had the keyboard focus.
    ///
    /// # Errors
    ///
    /// When it is not pressed within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    pub fn wait_key(&self, key: u32) -> Result<(), Error> {
        self.wait_input("a key press", |status| status.input.keys.contains(&key))
    }

    /// The keys pressed while it had the keyboard focus, by evdev code, in
    /// order.
    #[must_use]
    pub fn keys(&self) -> Vec<u32> {
        lock(&self.shared.status).input.keys.clone()
    }

    pub(crate) fn wait_input(
        &self,
        what: &str,
        done: impl Fn(&Status) -> bool,
    ) -> Result<(), Error> {
        let deadline = Deadline::patience();
        let mut status = lock(&self.shared.status);
        loop {
            if done(&status) {
                return Ok(());
            }
            let left = deadline.left();
            if left.is_zero() {
                return Err(Error::NotReceived {
                    title: self.shared.title.clone(),
                    what: what.to_owned(),
                });
            }
            status = self
                .shared
                .changed
                .wait_timeout(status, left)
                .map_or_else(|poisoned| poisoned.into_inner().0, |(status, _)| status);
        }
    }

    fn wait(
        &self,
        what: &str,
        done: impl Fn(&Status) -> Option<(i32, i32)>,
    ) -> Result<(i32, i32), Error> {
        let deadline = Deadline::patience();
        let mut status = lock(&self.shared.status);
        loop {
            if let Some(failure) = status.failure.take() {
                return Err(Error::Buffer(failure));
            }
            if let Some(size) = done(&status) {
                return Ok(size);
            }
            let left = deadline.left();
            if left.is_zero() {
                return Err(Error::NotDrawn {
                    title: self.shared.title.clone(),
                    what: what.to_owned(),
                    drawn: status.drawn,
                });
            }
            status = self
                .shared
                .changed
                .wait_timeout(status, left)
                .map_or_else(|poisoned| poisoned.into_inner().0, |(status, _)| status);
        }
    }

    fn commit(&self) -> Result<(), Error> {
        self.shared.surface.commit();
        self.connection.flush().map_err(Error::Send)
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        lock(&self.windows).remove(&self.shared.surface.id());
        // Before its toplevel, as the protocol asks.
        if let Some(decoration) = &self.decoration {
            decoration.destroy();
        }
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.shared.surface.destroy();
        drop(self.connection.flush());
    }
}

/// A buffer of `size` filled with `paint`, in a file of its own; the client
/// destroys it when the compositor releases it.
fn buffer(
    client: &Client,
    paint: &Paint,
    (width, height): (i32, i32),
    qh: &QueueHandle<Client>,
) -> Result<WlBuffer, ShmError> {
    // A configure never asks for a negative size; zero is the client's
    // choice, resolved before this.
    let size = (width.unsigned_abs(), height.unsigned_abs());
    let contents = paint.argb8888(size);
    let buffer = ShmBuffer::new(
        &client.shm,
        qh,
        size,
        size.0.saturating_mul(4),
        wl_shm::Format::Argb8888,
        contents.as_deref(),
        (),
    )?;
    Ok(buffer.into_buffer())
}

impl Dispatch<XdgToplevel, Arc<Shared>> for Client {
    fn event(
        _: &mut Self,
        toplevel: &XdgToplevel,
        event: xdg_toplevel::Event,
        shared: &Arc<Shared>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let fullscreen = states
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|state| u32::from_ne_bytes(*state))
                    .any(|state| state == u32::from(xdg_toplevel::State::Fullscreen));
                let mut status = lock(&shared.status);
                status.asked = (width, height);
                // Fullscreen once the compositor also says at what size: a
                // first configure may set the state before the size.
                status.asked_fullscreen = fullscreen && width > 0 && height > 0;
            }
            xdg_toplevel::Event::Close => {
                lock(&shared.status).close_requested = true;
                shared.changed.notify_all();
                // Obey, as an application would: the window goes. (Dropping
                // the `TestWindow` later destroys the rest; a request to the
                // destroyed toplevel is ignored.)
                toplevel.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgSurface, Arc<Shared>> for Client {
    fn event(
        client: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        shared: &Arc<Shared>,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let xdg_surface::Event::Configure { serial } = event else {
            return;
        };
        xdg_surface.ack_configure(serial);
        let mut status = lock(&shared.status);
        if status.close_requested {
            return;
        }
        let size = match status.asked {
            (width, height) if width > 0 && height > 0 => (width, height),
            _ => status.drawn.unwrap_or(DEFAULT_SIZE),
        };
        if status.drawn != Some(size) || status.drawn_fullscreen != status.asked_fullscreen {
            match buffer(client, &shared.paint, size, qh) {
                Ok(buffer) => {
                    shared.surface.attach(Some(&buffer), 0, 0);
                    shared.surface.damage(0, 0, size.0, size.1);
                    status.drawn = Some(size);
                    status.drawn_fullscreen = status.asked_fullscreen;
                }
                Err(failure) => status.failure = Some(failure),
            }
        }
        shared.surface.commit();
        shared.changed.notify_all();
    }
}
