//! The keyboard, as a test window hears it: which window has the keyboard
//! focus, and the keys pressed while it does, by their evdev codes.
//!
//! The client takes the seat's keyboard whenever the seat has one
//! ([`crate::seat`]): on a headless wlroots compositor that is only once a
//! virtual keyboard exists (`deskrinch-test-input`).

use std::sync::Arc;

use wayland_client::protocol::wl_keyboard::{self, KeyState, WlKeyboard};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::client::Client;
use crate::lock;
use crate::window::Shared;

/// The keyboard's state on the client's thread.
#[derive(Debug, Default)]
pub(crate) struct Keyboard {
    keyboard: Option<WlKeyboard>,
    /// The window with the keyboard focus.
    focus: Option<Arc<Shared>>,
}

impl Keyboard {
    /// Takes the seat's keyboard when it `has` one, and lets it go when it
    /// no longer does.
    pub(crate) fn follow(&mut self, seat: &WlSeat, has: bool, qh: &QueueHandle<Client>) {
        match (&self.keyboard, has) {
            (None, true) => self.keyboard = Some(seat.get_keyboard(qh, ())),
            (Some(keyboard), false) => {
                keyboard.release();
                self.keyboard = None;
                focus(self, None);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { surface, .. } => {
                let window = lock(&client.windows).get(&surface.id()).cloned();
                focus(&mut client.keyboard, window);
            }
            wl_keyboard::Event::Leave { .. } => focus(&mut client.keyboard, None),
            wl_keyboard::Event::Key {
                key,
                state: WEnum::Value(KeyState::Pressed),
                ..
            } => {
                if let Some(window) = &client.keyboard.focus {
                    lock(&window.status).input.keys.push(key);
                    window.changed.notify_all();
                }
            }
            _ => {}
        }
    }
}

/// Moves the keyboard focus to `window`.
fn focus(keyboard: &mut Keyboard, window: Option<Arc<Shared>>) {
    if let Some(old) = keyboard.focus.take() {
        lock(&old.status).input.keyboard = false;
        old.changed.notify_all();
    }
    if let Some(new) = &window {
        lock(&new.status).input.keyboard = true;
        new.changed.notify_all();
    }
    keyboard.focus = window;
}
