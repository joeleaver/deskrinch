//! What the shell's protocol crates (`deskrinch-window-management`,
//! `deskrinch-workspaces`) share: a Wayland connection of their own,
//! separate from any UI toolkit's, read and dispatched by one thread; and
//! the compositor's outputs, named and laid out.
//!
//! - [`Client`] is one connection to the compositor, with one thread that
//!   is its only reader and dispatches every queue on it. It is cheap to
//!   clone, and several crates may share it: each attaches a state of its
//!   own ([`Client::attach`]), which the thread dispatches on a queue of
//!   its own. A component using two of them, as the panel does, then holds
//!   one connection and one thread beside its toolkit's.
//! - [`Attachment`] keeps a state attached; dropping it detaches the state.
//! - [`Lifecycle`] is what every state does at the ends of its life: hear
//!   that the connection ended, once, with its [`EndCause`]; and, when its
//!   attachment is dropped, release what it bound.
//! - [`Globals`] is how it hears of the globals that appear and go after
//!   its setup: the client owns the connection's one `wl_registry`, and
//!   every state binds from it onto its own queue.
//! - [`Outputs`] binds `wl_output` and `zxdg_output_v1` into a crate's own
//!   state ([`OutputsHandler`]) and tracks each output's name and logical
//!   rectangle.
//!
//! No async runtime is needed, and nothing here depends on rinch.
//!
//! # One thread reads a connection
//!
//! Every queue on a connection is dispatched by the client's one thread,
//! which is also the only one ever to read the socket. No crate holds an
//! [`EventQueue`](wayland_client::EventQueue) once it is attached: its
//! setup runs on the client's thread, and it keeps only its queue's handle
//! (to send requests) and its [`Attachment`] (to flush them). The
//! connection itself is not handed out.
//!
//! A flush the socket will not take whole (the compositor is slow to read)
//! is not a failure: [`Attachment::flush`] says [`Flushed::Pending`], and
//! the client's thread waits until the socket is writable and sends the
//! rest. The connection ends only on a real error.
//!
//! The reason: with a thread per queue, each calling
//! [`EventQueue::blocking_dispatch`](wayland_client::EventQueue::blocking_dispatch),
//! a thread can wait forever for events that are already in its queue.
//! `blocking_dispatch` (wayland-client 0.31) does three things in turn: it
//! dispatches what its queue holds; it flushes; and it registers as a
//! reader (`prepare_read`) and waits on the socket. Its queue is not looked
//! at again between the first and the last step (libwayland's
//! `wl_display_prepare_read_queue` looks at its own internal queue, not at
//! the queues wayland-client sorts events into). If another queue's thread
//! reads the socket in between, and the events it reads include this
//! queue's, they land in this queue, and this thread waits on a socket that
//! has nothing more for it, until something unrelated arrives. That was
//! seen: a roundtrip on one queue waiting indefinitely, with its answer in
//! its queue, the socket empty, and the compositor idle. With one reader
//! there is no other thread to read in between.
//!
//! wayland-client's own documentation says the opposite, that a queue per
//! thread with `blocking_dispatch` on each "Just Work"s; it does not, with
//! either backend.
//!
//! One thread also costs least: a burst of events wakes one thread, and a
//! connection with several crates on it has one thread, not one per crate.
//! The handlers it runs only update a model under a lock and hand events
//! to their subscriber, so one crate's handlers do not hold up another's.
//!
//! # Ending
//!
//! A lost connection reaches every attached state's [`Lifecycle::end`]
//! exactly once ([`EndCause::ConnectionLost`]). A handler that panics (a
//! subscriber's, say, or a releasing state's) ends the connection for
//! every state ([`EndCause::HandlerPanicked`]): the panic is reported by
//! the panic hook where it happens, and no state is left silently
//! undispatched, since an undispatched state's objects would fill its queue
//! for as long as the connection lasts. The thread ends when the last
//! [`Client`] clone and the last [`Attachment`] are gone, releasing states
//! or not: the connection then closes at once.
//!
//! # Detaching
//!
//! Every object a state bound keeps sending it events until the compositor
//! is told to stop, and wayland-client keeps queueing them for the state's
//! queue as long as any of its objects lives. So a detached state is not
//! simply dropped. Its [`Lifecycle::release`] sends each protocol's
//! requests for stopping (a manager's `stop`, a handle's `destroy`,
//! [`Outputs::release`]), in the round it is detached; the thread keeps
//! dispatching it, as a releasing state whose handlers create and keep
//! nothing, and drops it once [`Lifecycle::released`] says nothing can
//! reach it: after a manager's `finished`, with every object destroyed.
//! [`Client::states`] counts both kinds, and the events releasing states
//! were handed.
//!
//! Some objects have no destructor: KWin's window-management and
//! virtual-desktop managers and its virtual desktops, and a `wl_seat` or
//! `wl_output` too old to be released. A state holding one is never
//! dropped: it is dispatched, and its events thrown away, for as long as
//! the connection lasts. So under KWin every detach on a shared
//! connection leaves a residue, on both sides, until the connection
//! closes: a component that would attach and detach repeatedly keeps its
//! state attached, or gives it a connection of its own (which closes when
//! its last handle goes, without waiting for anything).
//!
//! A compositor that stops reading for long enough, while the crates keep
//! sending, still ends the connection, and that limit is upstream's, not
//! one this crate can lift. Behind the socket's own kernel buffer, the
//! Rust backend holds unsent requests in a buffer of its own of 4096
//! bytes (`DEFAULT_MAX_BUFFER_SIZE` in wayland-backend's `src/rs/mod.rs`),
//! about 250 to 340 requests of the 12 to 16 bytes the crates send (a
//! `sync`, an activation); a request that finds it full while the socket
//! takes nothing is recorded as the connection's error
//! (`InnerBackend::send_request` in `src/rs/client_impl/mod.rs`).
//! libwayland's buffer is its own, 4096 bytes by default too.

mod attach;
mod client;
mod globals;
mod lifecycle;
mod outputs;
mod states;
mod thread;

pub use attach::{AttachError, Attachment, Detached, Flushed, Setup};
pub use client::{Client, ClientError};
pub use globals::Globals;
pub use lifecycle::{EndCause, Lifecycle, Remaining};
pub use outputs::{OutputGlobal, Outputs, OutputsHandler, Rect, layout_available, names_available};
pub use states::StateCounts;
