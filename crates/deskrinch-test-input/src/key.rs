//! Keys, by the Linux evdev code both input mechanisms send, and the text
//! that can be typed with them.

/// A key on a US keyboard, by its evdev code (`KEY_*` in
/// `linux/input-event-codes.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(u32);

impl Key {
    /// Escape.
    pub const ESCAPE: Self = Self(1);
    /// Backspace.
    pub const BACKSPACE: Self = Self(14);
    /// Tab.
    pub const TAB: Self = Self(15);
    /// Enter (the main one).
    pub const ENTER: Self = Self(28);
    /// The space bar.
    pub const SPACE: Self = Self(57);
    /// Up.
    pub const UP: Self = Self(103);
    /// Left.
    pub const LEFT: Self = Self(105);
    /// Right.
    pub const RIGHT: Self = Self(106);
    /// Down.
    pub const DOWN: Self = Self(108);
    /// The left Control.
    pub const LEFT_CTRL: Self = Self(29);
    /// The left Shift.
    pub const LEFT_SHIFT: Self = Self(42);
    /// The left Alt.
    pub const LEFT_ALT: Self = Self(56);
    /// The left logo key (Super, Meta).
    pub const LEFT_META: Self = Self(125);

    /// The modifier mask the key sets while held, in the virtual keyboard's
    /// keymap (xkb's real modifiers: Shift, Lock, Control, Mod1 for Alt, ...,
    /// Mod4 for the logo key); `None` for a key that is not a modifier.
    #[must_use]
    pub fn modifier_mask(self) -> Option<u32> {
        match self {
            Self::LEFT_SHIFT => Some(1 << 0),
            Self::LEFT_CTRL => Some(1 << 2),
            Self::LEFT_ALT => Some(1 << 3),
            Self::LEFT_META => Some(1 << 6),
            _ => None,
        }
    }

    /// The key's evdev code.
    #[must_use]
    pub fn code(self) -> u32 {
        self.0
    }

    /// The key that types `c` on a US layout without a modifier: a
    /// lowercase letter, a digit, a space, `-`, `.` or `/`.
    #[must_use]
    pub fn typing(c: char) -> Option<Self> {
        const LETTERS: [u32; 26] = [
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47,
            17, 45, 21, 44,
        ];
        let code = match c {
            'a'..='z' => LETTERS[usize::from(c as u8 - b'a')],
            '1'..='9' => u32::from(c as u8 - b'1') + 2,
            '0' => 11,
            ' ' => Self::SPACE.0,
            '-' => 12,
            '.' => 52,
            '/' => 53,
            _ => return None,
        };
        Some(Self(code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_modifiers_have_a_mask_each_its_own() {
        let masks: Vec<u32> = [
            Key::LEFT_SHIFT,
            Key::LEFT_CTRL,
            Key::LEFT_ALT,
            Key::LEFT_META,
        ]
        .iter()
        .map(|key| key.modifier_mask().unwrap())
        .collect();
        assert_eq!(masks, [1, 4, 8, 64]);
        assert_eq!(Key::SPACE.modifier_mask(), None);
    }

    #[test]
    fn types_letters_digits_and_a_few_signs() {
        // KEY_A, KEY_Q, KEY_Z, KEY_M; KEY_1, KEY_9, KEY_0.
        let codes: Vec<u32> = "aqzm190 -./"
            .chars()
            .map(|c| Key::typing(c).unwrap().code())
            .collect();
        assert_eq!(codes, [30, 16, 44, 50, 2, 10, 11, 57, 12, 52, 53]);
        for c in ['A', '!', 'é', '\n'] {
            assert_eq!(Key::typing(c), None, "{c:?}");
        }
    }
}
