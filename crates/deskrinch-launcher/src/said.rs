//! The two lines the launcher writes, at info, for what reads its log from
//! outside: a person, its compositor tests, and `cargo xtask measure
//! --type`, which is given these words as its `--keyboard-line` and
//! `--query-line`. One home for each wording, which the launcher logs with
//! and its tests read.
//!
//! # What "the field has the keyboard" means
//!
//! A key typed after the line reaches the query field. The launcher writes
//! it once per show, the first time both hold: its window has the keyboard
//! of the compositor (`rinch::window::window_focused`), and the field is the
//! element holding the keyboard in its document (`NodeHandle::active_element`
//! is the field).
//!
//! The show asks for the field's focus as it asks for the window, and rinch
//! applies a focus request on a later turn of its event loop, while the
//! compositor can give the window the keyboard only once the window is on
//! the screen, a frame and a round trip later. So the field is usually
//! focused first. That order is usual, not something rinch promises, so
//! the launcher does not rely on it. It checks both when the window gains
//! the keyboard, and if only the window is focused it writes nothing: a
//! reader waiting for the line then waits in vain, rather than typing into
//! nothing. (rinch says nothing reactive when an element gains focus, so a
//! field focused after the window would go unsaid until the next show.)
//!
//! # The query at the hide
//!
//! At each hide the launcher says how many characters its query had, never
//! the query: the proof that every key typed reached the field.

/// The line the launcher writes once per show when its field has the
/// keyboard: a key typed after it reaches the field.
pub const FIELD_HAS_KEYBOARD: &str = "the launcher's field has the keyboard";

/// The line the launcher writes at each hide, `{}` standing for how many
/// characters its query had ([`query_at_hide`]).
pub const QUERY_AT_HIDE: &str = "hidden with a query of {} characters";

/// [`QUERY_AT_HIDE`] for a query of `chars` characters.
#[must_use]
pub fn query_at_hide(chars: usize) -> String {
    QUERY_AT_HIDE.replace("{}", &chars.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_at_hide_says_its_length_where_the_wording_holds_a_place() {
        assert_eq!(query_at_hide(5), "hidden with a query of 5 characters");
        assert_eq!(QUERY_AT_HIDE.matches("{}").count(), 1);
    }
}
