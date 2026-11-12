//! [`Input`]: the connection, the mechanism the compositor offers, and the
//! keyboard ([`crate::pointer`] has the pointer).

use std::io::{Seek, Write};
use std::os::fd::AsFd;
use std::time::Instant;

use deskrinch_test_client::{Compositor, TestClient};
use deskrinch_test_wayland::{Deadline, Globals, roundtrip};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, EventQueue, Proxy, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

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
/// A key's or a button's state as both mechanisms send it.
pub(crate) const RELEASED: u32 = 0;
pub(crate) const PRESSED: u32 = 1;

/// The devices input goes through.
#[derive(Debug)]
pub(crate) enum Backend {
    /// wlroots' virtual keyboard and virtual pointer: devices of the
    /// seat's. The pointer is `None` when the compositor has no virtual
    /// pointer manager.
    Wlroots {
        keyboard: ZwpVirtualKeyboardV1,
        pointer: Option<ZwlrVirtualPointerV1>,
    },
    /// KWin's fake input, authenticated: one device for keys and pointer.
    Kwin(OrgKdeKwinFakeInput),
}

impl Backend {
    /// Destroys the devices: the compositor removes them as it handles the
    /// request, not when it notices the connection close. KWin's fake input
    /// is destroyed so from version 5; before, with the connection.
    pub(crate) fn destroy(&self) {
        match self {
            Self::Wlroots { keyboard, pointer } => {
                keyboard.destroy();
                if let Some(pointer) = pointer {
                    pointer.destroy();
                }
            }
            Self::Kwin(fake) => {
                if fake.version() >= 5 {
                    fake.destroy();
                }
            }
        }
    }
}

/// The queue's state: nothing is dispatched to it.
#[derive(Debug)]
pub(crate) struct State;

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
delegate_noop!(State: ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ZwlrVirtualPointerV1);
delegate_noop!(State: OrgKdeKwinFakeInput);

/// Keys and pointer input into the nested session's compositor.
#[derive(Debug)]
pub struct Input {
    connection: Connection,
    queue: EventQueue<State>,
    pub(crate) backend: Backend,
    /// The origin of the virtual devices' timestamps.
    start: Instant,
    /// A client of its own, for where the outputs are in the layout
    /// ([`crate::layout`]).
    pub(crate) outputs: TestClient,
    /// The buttons pressed and not yet released, which a drop releases.
    pub(crate) held: Vec<crate::Button>,
}

impl Input {
    /// Connects to the compositor `WAYLAND_DISPLAY` names and makes the
    /// devices: a keyboard, and a pointer where the compositor can make one.
    /// Panics outside a nested session ([`Compositor::current`]).
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
            let pointers: Option<ZwlrVirtualPointerManagerV1> = globals.bind(&qh, 1..=2, ()).ok();
            let pointer =
                pointers.map(|pointers| pointers.create_virtual_pointer(Some(&seat), &qh, ()));
            Backend::Wlroots { keyboard, pointer }
        } else if globals.offers(OrgKdeKwinFakeInput::interface().name) {
            // `keyboard_key` came with version 4.
            let fake: OrgKdeKwinFakeInput = globals.bind(&qh, 4..=5, ()).map_err(Error::Bind)?;
            fake.authenticate("deskrinch tests".into(), "press keys in tests".into());
            Backend::Kwin(fake)
        } else {
            return Err(Error::NoMechanism);
        };
        let outputs = TestClient::connect().map_err(Error::Outputs)?;
        let mut input = Self {
            connection,
            queue,
            backend,
            start: Instant::now(),
            outputs,
            held: Vec::new(),
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

    /// Presses `key` while holding `modifiers` (such as [`Key::LEFT_CTRL`]),
    /// as a person types a chord: the modifiers pressed in order, the key
    /// pressed and released, the modifiers released in the reverse order.
    ///
    /// KWin reads the modifiers' state from the keys themselves. wlroots'
    /// virtual keyboard does not: it takes the state as the client sends it,
    /// so it is sent after each modifier key, as a keyboard's own state
    /// would change.
    ///
    /// # Errors
    ///
    /// [`Error::NotAModifier`] for a modifier that is not one, before
    /// anything is sent; otherwise as [`press`](Self::press).
    pub fn press_with(&mut self, modifiers: &[Key], key: Key) -> Result<(), Error> {
        let masks = modifiers
            .iter()
            .map(|&modifier| {
                modifier
                    .modifier_mask()
                    .ok_or(Error::NotAModifier(modifier))
            })
            .collect::<Result<Vec<u32>, _>>()?;
        let mut held = 0;
        for (&modifier, mask) in modifiers.iter().zip(&masks) {
            self.send(modifier, PRESSED);
            held |= mask;
            self.send_modifiers(held);
        }
        self.send(key, PRESSED);
        self.send(key, RELEASED);
        for (&modifier, mask) in modifiers.iter().zip(&masks).rev() {
            self.send(modifier, RELEASED);
            held &= !mask;
            self.send_modifiers(held);
        }
        self.settle()
    }

    /// Tells a wlroots compositor the modifiers held, by their masks in
    /// the keymap; KWin needs nothing.
    fn send_modifiers(&self, depressed: u32) {
        match &self.backend {
            Backend::Wlroots { keyboard, .. } => keyboard.modifiers(depressed, 0, 0, 0),
            Backend::Kwin(_) => {}
        }
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
            Backend::Wlroots { keyboard, .. } => keyboard.key(self.time(), key.code(), state),
            Backend::Kwin(fake) => fake.keyboard_key(key.code(), state),
        }
    }

    /// A timestamp for the virtual devices: milliseconds since connecting.
    pub(crate) fn time(&self) -> u32 {
        u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    /// Waits until the compositor has handled everything sent.
    pub(crate) fn settle(&mut self) -> Result<(), Error> {
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
