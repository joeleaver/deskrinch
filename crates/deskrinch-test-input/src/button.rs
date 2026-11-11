//! Pointer buttons, by the Linux evdev code both input mechanisms send.

/// A pointer button, by its evdev code (`BTN_*` in
/// `linux/input-event-codes.h`), which is also what a client's
/// `wl_pointer.button` event carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Button(u32);

impl Button {
    /// The primary button: `BTN_LEFT`.
    pub const PRIMARY: Self = Self(0x110);
    /// The secondary button: `BTN_RIGHT`.
    pub const SECONDARY: Self = Self(0x111);
    /// The middle button: `BTN_MIDDLE`.
    pub const MIDDLE: Self = Self(0x112);

    /// The button's evdev code.
    #[must_use]
    pub fn code(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_the_kernels() {
        assert_eq!(Button::PRIMARY.code(), 272);
        assert_eq!(Button::SECONDARY.code(), 273);
        assert_eq!(Button::MIDDLE.code(), 274);
    }
}
