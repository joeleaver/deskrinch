//! The connection's one `wl_registry`, owned by the client, and how an
//! attached state hears of the globals that come and go after its setup.

use wayland_client::globals::{Global, GlobalError, GlobalList, GlobalListContents};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, DispatchError, EventQueue, QueueHandle};

/// How an attached state hears of the globals that appear and go after its
/// setup: the client's thread calls these with the connection's one
/// registry. The globals there at setup are in
/// [`Setup::globals`](crate::Setup::globals) instead.
///
/// Both do nothing unless the state says otherwise: a state that binds
/// nothing later has nothing to do here.
pub trait Globals: Sized {
    /// `global` appeared. A state binds it from `registry` onto its own queue
    /// (`qh`) if it wants it.
    fn global_added(&mut self, _registry: &WlRegistry, _global: &Global, _qh: &QueueHandle<Self>) {}

    /// The global named `name` went away.
    fn global_removed(&mut self, _name: u32) {}
}

/// A registry event, as the client's thread hands it on.
#[derive(Debug, Clone)]
pub(crate) enum GlobalEvent {
    /// A global appeared.
    Added(Global),
    /// The global with this name went away.
    Removed(u32),
}

/// The registry queue's state: the events read since they were last taken.
#[derive(Debug, Default)]
struct Heard(Vec<GlobalEvent>);

impl Dispatch<WlRegistry, GlobalListContents> for Heard {
    fn event(
        heard: &mut Self,
        _: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => heard.0.push(GlobalEvent::Added(Global {
                name,
                interface,
                version,
            })),
            wl_registry::Event::GlobalRemove { name } => heard.0.push(GlobalEvent::Removed(name)),
            _ => {}
        }
    }
}

/// The connection's one registry, on a queue of its own: the list of
/// globals every setup binds from, and the events after it.
#[derive(Debug)]
pub(crate) struct Registry {
    globals: GlobalList,
    queue: EventQueue<Heard>,
    heard: Heard,
}

impl Registry {
    /// Makes the registry on `connection` and lists the globals, in one
    /// exchange. Run before the client's thread starts, when nothing reads
    /// the connection yet.
    pub(crate) fn new(connection: &Connection) -> Result<Self, GlobalError> {
        let (globals, queue) = wayland_client::globals::registry_queue_init(connection)?;
        Ok(Self {
            globals,
            queue,
            heard: Heard::default(),
        })
    }

    /// The globals, as the compositor has listed them so far.
    pub(crate) fn globals(&self) -> &GlobalList {
        &self.globals
    }

    /// The registry events read since they were last taken, in order. It
    /// reads nothing itself: what the client's thread has read is sorted
    /// here.
    pub(crate) fn take(&mut self) -> Result<Vec<GlobalEvent>, DispatchError> {
        self.queue.dispatch_pending(&mut self.heard)?;
        Ok(std::mem::take(&mut self.heard.0))
    }
}
