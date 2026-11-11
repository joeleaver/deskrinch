//! [`TestClient`]: one Wayland connection, dispatched on a thread of its
//! own, which may have several windows.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use deskrinch_test_wayland::{Deadline, Globals, WaitError, dispatch_until, roundtrip};

use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::WlOutput;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1;
use wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::{
    self, ZxdgToplevelDecorationV1,
};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;

use crate::keyboard::Keyboard;
use crate::output_info::OutputInfo;
use crate::pointer::Pointer;
use crate::seat::Windows;
use crate::window::{Shared, Status, TestWindow};
use crate::{Error, PATIENCE, Paint, lock, waited};

/// The client's thread's state.
#[derive(Debug)]
pub(crate) struct Client {
    pub(crate) shm: WlShm,
    /// Outputs by name, as `wl_output` names them.
    pub(crate) outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    /// What each output reports about itself.
    pub(crate) infos: Arc<Mutex<HashMap<ObjectId, OutputInfo>>>,
    /// The windows, by surface, for the keyboard's events.
    pub(crate) windows: Windows,
    /// The seat's keyboard, and which window has its focus.
    pub(crate) keyboard: Keyboard,
    /// The seat's pointer, and which window it is over.
    pub(crate) pointer: Pointer,
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
    /// `xdg-decoration`, where the compositor offers it: each window asks
    /// to draw its own decorations, that is none, so that it fills what the
    /// compositor gives it (sway and labwc otherwise draw borders and a
    /// title bar inside a maximised or tiled window's area).
    decorations: Option<ZxdgDecorationManagerV1>,
    outputs: Arc<Mutex<HashMap<String, WlOutput>>>,
    infos: Arc<Mutex<HashMap<ObjectId, OutputInfo>>>,
    windows: Windows,
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
        let deadline = Deadline::after(PATIENCE);
        // The globals are listed before the client's state exists, on a
        // queue of their own: the registry keeps its own list, and nothing
        // is dispatched to that queue's state.
        let mut listing = connection.new_event_queue();
        let globals = Globals::list(&connection, &mut listing, &mut Listing, deadline)
            .map_err(|error| waited(error, "listing the globals"))?;
        let mut queue = connection.new_event_queue();
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 1..=4, ()).map_err(Error::Bind)?;
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).map_err(Error::Bind)?;
        let wm_base: XdgWmBase = globals.bind(&qh, 1..=1, ()).map_err(Error::Bind)?;
        // Logical sizes, where the compositor offers them (all three do).
        let xdg_outputs: Option<ZxdgOutputManagerV1> = globals.bind(&qh, 1..=3, ()).ok();
        let decorations: Option<ZxdgDecorationManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        // The seat, where there is one: windows hear its keyboard and pointer.
        let _seat: Option<WlSeat> = globals.bind(&qh, 1..=7, ()).ok();
        let announced = globals.all();
        for global in &announced {
            if global.interface == WlOutput::interface().name {
                let output: WlOutput = globals.bind_global(global, global.version.min(4), &qh, ());
                if let Some(manager) = &xdg_outputs {
                    manager.get_xdg_output(&output, &qh, output.id());
                }
            }
        }
        let outputs = Arc::new(Mutex::new(HashMap::new()));
        let infos = Arc::new(Mutex::new(HashMap::new()));
        let windows = Windows::default();
        let mut client = Client {
            shm,
            outputs: Arc::clone(&outputs),
            infos: Arc::clone(&infos),
            windows: Arc::clone(&windows),
            keyboard: Keyboard::default(),
            pointer: Pointer::default(),
        };
        roundtrip(&connection, &mut queue, &mut client, deadline)
            .map_err(|error| waited(error, "learning the outputs"))?;

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
            decorations,
            outputs,
            infos,
            windows,
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

    /// What the output named `name` reports about itself, as of now.
    #[must_use]
    pub fn output_info(&self, name: &str) -> Option<OutputInfo> {
        let output = lock(&self.outputs).get(name)?.id();
        Some(lock(&self.infos).get(&output).copied().unwrap_or_default())
    }

    /// The scale `wl_output` reports for the output named `name`: a whole
    /// number, rounded up from a fractional scale (see [`output_info`] for
    /// its logical size, which tells them apart).
    ///
    /// [`output_info`]: Self::output_info
    #[must_use]
    pub fn output_scale(&self, name: &str) -> Option<i32> {
        self.output_info(name).map(|info| info.scale)
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
        lock(&self.windows).insert(surface.id(), Arc::clone(&shared));
        let xdg_surface = self
            .wm_base
            .get_xdg_surface(&surface, &self.queue, Arc::clone(&shared));
        let toplevel = xdg_surface.get_toplevel(&self.queue, Arc::clone(&shared));
        let decoration = self.decorations.as_ref().map(|manager| {
            let decoration = manager.get_toplevel_decoration(&toplevel, &self.queue, ());
            decoration.set_mode(zxdg_toplevel_decoration_v1::Mode::ClientSide);
            decoration
        });
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
            Arc::clone(&self.windows),
            toplevel,
            decoration,
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

/// How often the client's thread looks whether it is to stop, while the
/// compositor is quiet.
const WAKE: Duration = Duration::from_millis(100);

/// The client's thread: dispatches until told to stop, or the connection
/// fails. It never waits on the compositor longer than [`WAKE`], so
/// dropping the client ends it even when the compositor has stopped
/// answering.
fn run(mut queue: EventQueue<Client>, mut client: Client, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        match dispatch_until(&mut queue, &mut client, Deadline::after(WAKE), |_| false) {
            Err(WaitError::TimedOut(_)) => {}
            // `done` never holds, so any other end is the connection's.
            Ok(()) | Err(_) => return,
        }
    }
}

/// The state of the queue while the globals are listed: nothing is
/// dispatched to it.
#[derive(Debug)]
struct Listing;

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

delegate_noop!(Client: WlCompositor);
delegate_noop!(Client: ZxdgDecorationManagerV1);
// The mode the compositor settles on is its own business: a window asks for
// client-side decorations and draws none either way.
delegate_noop!(Client: ignore ZxdgToplevelDecorationV1);
delegate_noop!(Client: WlShmPool);
delegate_noop!(Client: ignore WlSurface);
delegate_noop!(Client: ignore WlShm);
delegate_noop!(Client: ignore WlCallback);
