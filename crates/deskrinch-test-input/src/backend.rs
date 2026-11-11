//! [`Input`]: the connection, and the mechanism the compositor offers.

use std::io::{Seek, Write};
use std::os::fd::AsFd;
use std::time::Instant;

use deskrinch_test_client::Compositor;
use deskrinch_test_wayland::{Deadline, Globals, roundtrip};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, EventQueue, Proxy, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

use crate::{Error, Key, PATIENCE};

/// A US keyboard, built from the system's xkb data, as the virtual
/// keyboard's keymap: what the compositor tells the window with the focus
/// the keys mean.
const KEYMAP: &str = "xkb_keymap {
    xkb_keycodes { include \"evdev+aliases(qwerty)\" };
    xkb_types { include \"complete\" };
    xkb_compatibility { include \"complete\" };
    xkb_symbols { include \"pc+us+inet(evdev)\" };
};
";

/// `wl_keyboard.keymap`'s format for an xkb keymap.
const XKB_V1: u32 = 1;
/// A key's state as both mechanisms send it.
const RELEASED: u32 = 0;
const PRESSED: u32 = 1;

/// The device keys go through.
#[derive(Debug)]
enum Backend {
    /// wlroots' virtual keyboard: a keyboard of the seat's.
    Wlroots(ZwpVirtualKeyboardV1),
    /// KWin's fake input, authenticated.
    Kwin(OrgKdeKwinFakeInput),
}

/// The queue's state: nothing is dispatched to it.
#[derive(Debug)]
struct State;

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
delegate_noop!(State: OrgKdeKwinFakeInput);

/// Keys into the nested session's compositor.
#[derive(Debug)]
pub struct Input {
    connection: Connection,
    queue: EventQueue<State>,
    backend: Backend,
    /// The origin of the virtual keyboard's timestamps.
    start: Instant,
}

impl Input {
    /// Connects to the compositor `WAYLAND_DISPLAY` names and makes the
    /// device. Panics outside a nested session
    /// ([`Compositor::current`]).
    ///
    /// # Errors
    ///
    /// When there is no compositor, it offers neither mechanism
    /// ([`Error::NoMechanism`]), or it does not answer within
    /// [`PATIENCE`].
    pub fn connect() -> Result<Self, Error> {
        let _nested = Compositor::current();
        let connection = Connection::connect_to_env().map_err(Error::Connect)?;
        let deadline = Deadline::after(PATIENCE);
        let mut queue = connection.new_event_queue();
        let globals =
            Globals::list(&connection, &mut queue, &mut State, deadline).map_err(Error::Wait)?;
        let qh = queue.handle();
        let backend = if globals.offers(ZwpVirtualKeyboardManagerV1::interface().name) {
            let manager: ZwpVirtualKeyboardManagerV1 =
                globals.bind(&qh, 1..=1, ()).map_err(Error::Bind)?;
            let seat: WlSeat = globals.bind(&qh, 1..=7, ()).map_err(Error::Bind)?;
            let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());
            let keymap = keymap().map_err(Error::Keymap)?;
            let size = u32::try_from(KEYMAP.len() + 1).unwrap_or(u32::MAX);
            keyboard.keymap(XKB_V1, keymap.as_fd(), size);
            Backend::Wlroots(keyboard)
        } else if globals.offers(OrgKdeKwinFakeInput::interface().name) {
            // `keyboard_key` came with version 4.
            let fake: OrgKdeKwinFakeInput = globals.bind(&qh, 4..=5, ()).map_err(Error::Bind)?;
            fake.authenticate("deskrinch tests".into(), "press keys in tests".into());
            Backend::Kwin(fake)
        } else {
            return Err(Error::NoMechanism);
        };
        let mut input = Self {
            connection,
            queue,
            backend,
            start: Instant::now(),
        };
        input.settle()?;
        Ok(input)
    }

    /// Presses `key` and releases it.
    ///
    /// # Errors
    ///
    /// When the compositor does not answer within [`PATIENCE`].
    pub fn press(&mut self, key: Key) -> Result<(), Error> {
        self.send(key, PRESSED);
        self.send(key, RELEASED);
        self.settle()
    }

    /// Types `text`, one key after another.
    ///
    /// # Errors
    ///
    /// [`Error::Untypable`] for a character [`Key::typing`] has no key
    /// for, before anything is typed; otherwise as [`press`](Self::press).
    pub fn type_text(&mut self, text: &str) -> Result<(), Error> {
        let keys = text
            .chars()
            .map(|c| Key::typing(c).ok_or(Error::Untypable(c)))
            .collect::<Result<Vec<_>, _>>()?;
        for key in keys {
            self.send(key, PRESSED);
            self.send(key, RELEASED);
        }
        self.settle()
    }

    fn send(&self, key: Key, state: u32) {
        match &self.backend {
            Backend::Wlroots(keyboard) => {
                let time = u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX);
                keyboard.key(time, key.code(), state);
            }
            Backend::Kwin(fake) => fake.keyboard_key(key.code(), state),
        }
    }

    /// Waits until the compositor has handled everything sent.
    fn settle(&mut self) -> Result<(), Error> {
        roundtrip(
            &self.connection,
            &mut self.queue,
            &mut State,
            Deadline::after(PATIENCE),
        )
        .map_err(Error::Wait)
    }
}

/// The keymap in a file of its own, ended by a NUL as the protocol wants.
fn keymap() -> std::io::Result<std::fs::File> {
    let mut file = tempfile::tempfile()?;
    file.write_all(KEYMAP.as_bytes())?;
    file.write_all(&[0])?;
    file.rewind()?;
    Ok(file)
}
