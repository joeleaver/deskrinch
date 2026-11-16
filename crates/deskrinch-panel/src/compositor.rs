//! The panel's one Wayland connection beside rinch's own, shared by the
//! widgets that follow the compositor: the taskbar's windows and the
//! switcher's workspaces. It is made on a thread of its own when the panel
//! starts, off the first frame's path, and handed to each follower in turn,
//! which attaches its state to it (`deskrinch-window-management`'s and
//! `deskrinch-workspaces`' `connect_with`): one connection and one thread
//! that reads it, instead of one of each per widget.
//!
//! A shared connection shares its fate: a protocol error raised by either
//! follower, or the compositor going away, ends both streams, and each
//! reports it as its own.

use deskrinch_component::report;
use deskrinch_wayland_client::Client;

/// What a follower does with the connection: attach to it, or, given
/// `None` when no connection could be made (reported once, here), show
/// nothing.
pub(crate) type Follow = Box<dyn FnOnce(Option<&Client>) + Send>;

/// Starts making the connection on a thread of its own and handing it to
/// each of `follows`, in order; returns at once.
pub(crate) fn start(follows: Vec<Follow>) {
    let spawned = std::thread::Builder::new()
        .name("compositor-connect".into())
        .spawn(move || {
            let client = match Client::connect() {
                Ok(client) => Some(client),
                Err(error) => {
                    report::error(
                        "connecting to the compositor for its windows and workspaces",
                        &error,
                    );
                    None
                }
            };
            for follow in follows {
                follow(client.as_ref());
            }
        });
    if let Err(error) = spawned {
        report::error(
            "starting the thread that connects to the compositor for its windows and \
             workspaces",
            &error,
        );
    }
}
