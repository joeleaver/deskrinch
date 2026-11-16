//! The two lines the launcher writes, at info, for what reads its log from
//! outside: a person, its compositor tests, and `cargo xtask measure
//! --type`, which is given these words as its `--keyboard-line` and
//! `--query-line`. One home for each wording, which the launcher logs with
//! and its tests read.
//!
//! # What "the launcher has the keyboard" means
//!
//! The launcher's window has the compositor's keyboard
//! (`rinch::window::window_focused`): written once per show, when it gains
//! it. It does not say that its query field holds the keyboard inside the
//! window, which is what a key typed needs.
//!
//! The show asks for the field's focus as it asks for the window, and rinch
//! applies the request on a later turn of its event loop. Usually that is
//! before the compositor gives the window the keyboard, which it can do
//! only once the window is on the screen, but nothing guarantees the order.
//! Under labwc, a launcher started shown (`--show`) is given the keyboard
//! first, and its field is focused some 20 ms later. rinch can say which
//! element holds the keyboard when asked (`NodeHandle::active_element`), but
//! says nothing when that changes, so the launcher cannot write the line at
//! the moment the field gains the keyboard. Until rinch says so, the line
//! is the window's.
//!
//! # The query at the hide
//!
//! At each hide the launcher says how many characters its query had, never
//! the query. That is the proof that every key typed reached the field,
//! whatever the order above.

/// The line the launcher writes once per show when its window gains the
/// keyboard.
pub const HAS_KEYBOARD: &str = "the launcher has the keyboard";

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
