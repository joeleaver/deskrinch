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
//! - [`Ended`] is how an attached state hears that the connection ended,
//!   once, with its [`EndCause`].
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
//! A lost connection reaches every attached state's [`Ended::end`] exactly
//! once ([`EndCause::ConnectionLost`]). A handler that panics (a
//! subscriber's, say) ends the connection for every state
//! ([`EndCause::HandlerPanicked`]): the panic is reported by the panic hook
//! where it happens, and no state is left silently undispatched. The thread
//! ends when the last [`Client`] clone and the last [`Attachment`] are
//! gone.
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
mod ended;
mod globals;
mod outputs;
mod thread;

pub use attach::{AttachError, Attachment, Detached, Flushed, Setup};
pub use client::{Client, ClientError};
pub use ended::{EndCause, Ended};
pub use globals::Globals;
pub use outputs::{OutputGlobal, Outputs, OutputsHandler, Rect, layout_available, names_available};
