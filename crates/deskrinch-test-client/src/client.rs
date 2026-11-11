//! [`TestClient`]: one Wayland connection, dispatched on a thread of its
//! own, which may have several windows.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use crate::window::{Shared, Status, TestWindow};
use crate::{Error, Paint, lock};

/// The client's thread's state.
#[derive(Debug)]
pub(crate) struct Client {
    pub(crate) shm: WlShm,
    /// Outputs by name, as `wl_output` names them.
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    /// Each output's integer scale, as `wl_output` reports it.
    scales: Arc<Mutex<HashMap<ObjectId, i32>>>,
}

/// One Wayland client, which may have several windows.
///
/// It is its own connection to `WAYLAND_DISPLAY`, so the compositor sees a
/// separate client, dispatched on a thread of its own so that it answers
/// pings and configures like a live application.
#[derive(Debug)]
pub struct TestClient {
    connection: Connection,
    queue: QueueHandle<Client>,
    compositor: WlCompositor,
    wm_base: XdgWmBase,
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    scales: Arc<Mutex<HashMap<ObjectId, i32>>>,
    /// The interfaces of the globals the compositor announced.
    globals: Vec<String>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestClient {
    /// Connects to the compositor `WAYLAND_DISPLAY` names and learns its
    /// outputs.
    ///
    /// # Errors
    ///
    /// When there is no compositor, or it lacks `wl_compositor`, `wl_shm`
    /// or `xdg_wm_base`.
    pub fn connect() -> Result<Self, Error> {
        Self::with(Connection::connect_to_env().map_err(Error::Connect)?)
    }

    /// Connects to the compositor listening on the Wayland socket at
    /// `socket`, for code that sets a session up rather than runs in it.
    ///
    /// # Errors
    ///
    /// As [`connect`](Self::connect).
    pub fn connect_to(socket: &Path) -> Result<Self, Error> {
        let stream = UnixStream::connect(socket).map_err(|source| Error::Socket {
            path: socket.to_owned(),
            source,
        })?;
        Self::with(Connection::from_socket(stream).map_err(Error::Connect)?)
    }

    fn with(connection: Connection) -> Result<Self, Error> {
        let (globals, mut queue) =
            registry_queue_init::<Client>(&connection).map_err(Error::Globals)?;
        let qh = queue.handle();
        let bind_error = |interface| move |source| Error::Bind { interface, source };
        let compositor: WlCompositor = globals
            .bind(&qh, 1..=4, ())
            .map_err(bind_error("wl_compositor"))?;
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).map_err(bind_error("wl_shm"))?;
        let wm_base: XdgWmBase = globals
            .bind(&qh, 1..=1, ())
            .map_err(bind_error("xdg_wm_base"))?;
        let announced = globals.contents().clone_list();
        for global in &announced {
            if global.interface == WlOutput::interface().name {
                globals.registry().bind::<WlOutput, _, _>(
                    global.name,
                    global.version.min(4),
                    &qh,
                    (),
                );
            }
        }
        let outputs = Arc::new(Mutex::new(HashMap::new()));
        let scales = Arc::new(Mutex::new(HashMap::new()));
        let mut client = Client {
            shm,
            outputs: Arc::clone(&outputs),
            scales: Arc::clone(&scales),
        };
        queue.roundtrip(&mut client).map_err(Error::Dispatch)?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let stop = Arc::clone(&stop);
            move || run(queue, client, &stop)
        });
        Ok(Self {
            connection,
            queue: qh,
            compositor,
            wm_base,
            outputs,
            scales,
            globals: announced
                .into_iter()
                .map(|global| global.interface)
                .collect(),
            stop,
            thread: Some(thread),
        })
    }

    /// Whether the compositor offered this client a global of `interface`
    /// when it connected.
    #[must_use]
    pub fn offers(&self, interface: &str) -> bool {
        self.globals.iter().any(|offered| offered == interface)
    }

    /// The names of the compositor's outputs, sorted.
    #[must_use]
    pub fn output_names(&self) -> Vec<String> {
        let mut names: Vec<_> = lock(&self.outputs).keys().cloned().collect();
        names.sort();
        names
    }

    /// The scale `wl_output` reports for the output named `name`: a whole
    /// number, rounded up from a fractional scale.
    #[must_use]
    pub fn output_scale(&self, name: &str) -> Option<i32> {
        let output = lock(&self.outputs).get(name)?.id();
        lock(&self.scales).get(&output).copied()
    }

    /// Opens a transparent window and returns once it has drawn its first
    /// buffer.
    ///
    /// # Errors
    ///
    /// When the compositor does not configure it within
    /// [`PATIENCE`](crate::PATIENCE), or the connection fails.
    pub fn open(&self, title: &str, app_id: &str) -> Result<TestWindow, Error> {
        self.open_window(title, app_id, None, Paint::Transparent, None)
    }

    /// Opens a window that fills itself with `paint`, fullscreen on the
    /// output named `fullscreen` if one is given (from its first configure,
    /// so it is never drawn at another size first), and returns once it has
    /// drawn its first buffer.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open), and when there is no output so named.
    pub fn open_painted(
        &self,
        title: &str,
        app_id: &str,
        paint: Paint,
        fullscreen: Option<&str>,
    ) -> Result<TestWindow, Error> {
        self.open_window(title, app_id, None, paint, fullscreen)
    }

    /// Opens a transparent window that belongs to `parent`, like a dialog.
    ///
    /// # Errors
    ///
    /// As [`open`](Self::open).
    pub fn open_child(
        &self,
        parent: &TestWindow,
        title: &str,
        app_id: &str,
    ) -> Result<TestWindow, Error> {
        self.open_window(
            title,
            app_id,
            Some(parent.toplevel()),
            Paint::Transparent,
            None,
        )
    }

    fn open_window(
        &self,
        title: &str,
        app_id: &str,
        parent: Option<&XdgToplevel>,
        paint: Paint,
        fullscreen: Option<&str>,
    ) -> Result<TestWindow, Error> {
        let fullscreen = fullscreen
            .map(|name| {
                lock(&self.outputs)
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Error::UnknownOutput(name.to_owned()))
            })
            .transpose()?;
        let surface = self.compositor.create_surface(&self.queue, ());
        let shared = Arc::new(Shared {
            title: title.to_owned(),
            surface: surface.clone(),
            paint,
            status: Mutex::new(Status::default()),
            changed: Condvar::new(),
        });
        let xdg_surface = self
            .wm_base
            .get_xdg_surface(&surface, &self.queue, Arc::clone(&shared));
        let toplevel = xdg_surface.get_toplevel(&self.queue, Arc::clone(&shared));
        toplevel.set_title(title.to_owned());
        toplevel.set_app_id(app_id.to_owned());
        toplevel.set_parent(parent);
        if let Some(output) = &fullscreen {
            toplevel.set_fullscreen(Some(output));
        }
        surface.commit();
        self.connection.flush().map_err(Error::Send)?;

        let window = TestWindow::new(
            self.connection.clone(),
            Arc::clone(&self.outputs),
            toplevel,
            xdg_surface,
            shared,
        );
        window.wait_drawn("its first buffer", |_| true)?;
        Ok(window)
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wakes the thread, which then sees `stop`.
        self.connection.display().sync(&self.queue, ());
        // The compositor may be gone already when a test fails or ends it.
        drop(self.connection.flush());
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn run(mut queue: EventQueue<Client>, mut client: Client, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        if queue.blocking_dispatch(&mut client).is_err() {
            return;
        }
    }
}

impl Dispatch<WlOutput, ()> for Client {
    fn event(
        client: &mut Self,
        output: &WlOutput,
        event: wl_output::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Name { name } => {
                lock(&client.outputs).insert(name, output.clone());
            }
            wl_output::Event::Scale { factor } => {
                lock(&client.scales).insert(output.id(), factor);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, ()> for Client {
    fn event(
        _: &mut Self,
        buffer: &WlBuffer,
        event: wl_buffer::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Each buffer is attached once; when the compositor lets go of it,
        // it is done with.
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

impl Dispatch<XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Client {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(Client: WlCompositor);
delegate_noop!(Client: WlShmPool);
delegate_noop!(Client: ignore WlSurface);
delegate_noop!(Client: ignore WlShm);
delegate_noop!(Client: ignore WlCallback);
