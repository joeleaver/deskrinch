//! The outputs a window is on, as its surface hears them
//! (`wl_surface.enter` and `leave`): where the compositor put it, which a
//! test asks rather than guessing (sway opens a window on the focused
//! output, wherever an earlier test left the focus).
//!
//! An output is named by the `wl_output`s the client bound when it
//! connected: one connected later (an output turned off and on again is a
//! new global) is not one of them, and a window that enters it does not
//! name it. A test that needs it connects a new client.

use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::client::Client;
use crate::window::TestWindow;
use crate::{Error, lock};

impl Dispatch<WlSurface, ()> for Client {
    fn event(
        client: &mut Self,
        surface: &WlSurface,
        event: wl_surface::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let (output, entered) = match event {
            wl_surface::Event::Enter { output } => (output, true),
            wl_surface::Event::Leave { output } => (output, false),
            _ => return,
        };
        // A layer surface is no window: what it is on is not asked.
        let Some(window) = lock(&client.windows).get(&surface.id()).cloned() else {
            return;
        };
        let Some(name) = lock(&client.outputs)
            .iter()
            .find(|(_, known)| known.id() == output.id())
            .map(|(name, _)| name.clone())
        else {
            return;
        };
        let mut status = lock(&window.status);
        status.outputs.retain(|on| *on != name);
        if entered {
            status.outputs.push(name);
        }
        drop(status);
        window.changed.notify_all();
    }
}

impl TestWindow {
    /// The outputs it is on now, by name, in the order it entered them:
    /// those the client knows (see the module's documentation).
    #[must_use]
    pub fn outputs(&self) -> Vec<String> {
        lock(&self.shared.status).outputs.clone()
    }

    /// Waits until it is on an output, and returns the first it entered of
    /// those it is on.
    ///
    /// # Errors
    ///
    /// When it enters none within [`PATIENCE`](deskrinch_test_wait::PATIENCE).
    pub fn wait_output(&self) -> Result<String, Error> {
        self.wait_input("an output's enter", |status| !status.outputs.is_empty())?;
        self.outputs()
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotReceived {
                title: self.shared.title.clone(),
                what: "an output's enter".to_owned(),
            })
    }
}
