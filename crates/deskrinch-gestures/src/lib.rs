//! Pointer gestures made of presses and releases, independent of any
//! toolkit: a component's model hears the presses and releases on its
//! targets (the results of a launcher, the buttons of a taskbar) and asks
//! what they add up to.
//!
//! - [`Click`]: a press and a release of the same button on the same target
//!   is a click on that target; anything else is none.
//!
//! The crate depends on nothing, so a model that has no UI can use it.

/// A click in the making: the press held, until the release says whether
/// it was a click.
///
/// `Target` is what can be clicked (a result, a window's button); `Button`
/// is which button, a small value (`()` for a model that hears only one). A press and a
/// release of the same button on the same target is a click on it. A
/// release on another target, or on none, is no click, and spends the
/// press. A release of another button than the one held is no click, and
/// leaves the press held, since that button is still down. A new press
/// replaces the one held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click<Target, Button = ()> {
    held: Option<(Option<Target>, Button)>,
}

impl<Target, Button> Default for Click<Target, Button> {
    fn default() -> Self {
        Self { held: None }
    }
}

impl<Target: PartialEq, Button: Copy + PartialEq> Click<Target, Button> {
    /// Nothing pressed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `button` went down on `on`, or (`None`) on no target.
    pub fn press(&mut self, on: Option<Target>, button: Button) {
        self.held = Some((on, button));
    }

    /// `button` came up on `on`, or on no target: the target clicked, if
    /// the press and this release make a click.
    pub fn release(&mut self, on: Option<Target>, button: Button) -> Option<Target> {
        let (pressed, held) = self.held.take()?;
        if held != button {
            self.held = Some((pressed, held));
            return None;
        }
        match (pressed, on) {
            (Some(pressed), Some(released)) if pressed == released => Some(released),
            _ => None,
        }
    }

    /// Whether a press is held.
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Button {
        Primary,
        Middle,
    }

    #[test]
    fn a_press_and_a_release_on_one_target_is_a_click_on_it() {
        let mut click = Click::new();
        click.press(Some("a"), Button::Primary);
        assert!(click.is_pressed());
        assert_eq!(click.release(Some("a"), Button::Primary), Some("a"));
        assert!(!click.is_pressed(), "the press is spent");
        assert_eq!(click.release(Some("a"), Button::Primary), None);
    }

    #[test]
    fn a_release_on_another_target_or_none_is_no_click_and_spends_the_press() {
        let mut click = Click::new();
        click.press(Some("a"), Button::Primary);
        assert_eq!(click.release(Some("b"), Button::Primary), None);
        assert!(!click.is_pressed());
        click.press(Some("a"), Button::Primary);
        assert_eq!(click.release(None, Button::Primary), None);
        click.press(None, Button::Primary);
        assert_eq!(click.release(Some("a"), Button::Primary), None);
    }

    #[test]
    fn a_release_of_another_button_leaves_the_press_held() {
        let mut click = Click::new();
        click.press(Some("a"), Button::Primary);
        assert_eq!(click.release(Some("a"), Button::Middle), None);
        assert!(click.is_pressed());
        assert_eq!(click.release(Some("a"), Button::Primary), Some("a"));
    }

    #[test]
    fn a_new_press_replaces_the_one_held() {
        let mut click = Click::new();
        click.press(Some("a"), Button::Primary);
        click.press(Some("b"), Button::Middle);
        assert_eq!(click.release(Some("a"), Button::Primary), None);
        assert_eq!(click.release(Some("b"), Button::Middle), Some("b"));
    }

    #[test]
    fn one_button_needs_no_button_type() {
        let mut click: Click<u32> = Click::new();
        click.press(Some(7), ());
        assert_eq!(click.release(Some(7), ()), Some(7));
    }
}
