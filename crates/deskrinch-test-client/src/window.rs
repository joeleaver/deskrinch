//! [`TestWindow`]: a mapped `xdg_toplevel` that acknowledges each configure
//! and draws its [`Paint`] at the size asked for. A close request is
//! recorded and obeyed: the toplevel is destroyed, as an application would
//! destroy it.

use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};

use crate::client::Client;
use crate::{Error, PATIENCE, Paint, lock};

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
    failure: Option<String>,
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
    toplevel: XdgToplevel,
    xdg_surface: XdgSurface,
    shared: Arc<Shared>,
}

impl TestWindow {
    pub(crate) fn new(
        connection: Connection,
        outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
        toplevel: XdgToplevel,
        xdg_surface: XdgSurface,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            connection,
            outputs,
            toplevel,
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
    /// When no such buffer is drawn within [`PATIENCE`], or a buffer could
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

    fn wait(
        &self,
        what: &str,
        done: impl Fn(&Status) -> Option<(i32, i32)>,
    ) -> Result<(i32, i32), Error> {
        let deadline = Instant::now() + PATIENCE;
        let mut status = lock(&self.shared.status);
        loop {
            if let Some(failure) = &status.failure {
                return Err(Error::Buffer(failure.clone()));
            }
            if let Some(size) = done(&status) {
                return Ok(size);
            }
            let left = deadline.saturating_duration_since(Instant::now());
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
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.shared.surface.destroy();
        drop(self.connection.flush());
    }
}

/// A buffer of `size` filled with `paint`, in a file of its own.
fn buffer(
    client: &Client,
    paint: &Paint,
    (width, height): (i32, i32),
    qh: &QueueHandle<Client>,
) -> Result<WlBuffer, String> {
    let stride = width.checked_mul(4).ok_or("the width overflows")?;
    let size = stride.checked_mul(height).ok_or("the size overflows")?;
    let unsigned = |value: i32| u32::try_from(value).map_err(|_| "a negative size".to_owned());
    let file: File = tempfile::tempfile().map_err(|error| format!("buffer file: {error}"))?;
    file.set_len(u64::from(unsigned(size)?))
        .map_err(|error| format!("sizing the buffer: {error}"))?;
    if let Some(bytes) = paint.argb8888((unsigned(width)?, unsigned(height)?)) {
        file.write_all_at(&bytes, 0)
            .map_err(|error| format!("painting the buffer: {error}"))?;
    }
    let pool = client.shm.create_pool(file.as_fd(), size, qh, ());
    let buffer = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
    pool.destroy();
    Ok(buffer)
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
