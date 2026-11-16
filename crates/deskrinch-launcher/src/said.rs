//! The two lines the launcher writes, at info, for what reads its log from
//! outside: a person, its compositor tests, and `cargo xtask measure
//! --type`, which is given these words as its `--keyboard-line` and
//! `--query-line`. One home for each wording, which the launcher logs with
//! and its tests read.
//!
//! # What "the launcher has the keyboard" means
//!
//! The launcher's query field holds the keyboard: the field is focused and
//! its window has the compositor's keyboard, which is what a key typed
//! needs. The line is written from the field's own `focus` event, which
//! rinch fires at exactly that moment, once per show: the first time in a
//! show that the field gains the keyboard.
//!
//! The order is rinch's. The show asks for the field's focus as it asks
//! for the window, and rinch applies that request before the window's next
//! frame, and before it tells the window it has the keyboard; the field's
//! `focus` event then comes with the keyboard. So a key typed after the
//! line reaches the field, whichever comes first on the compositor's side.
//!
//! # The query at the hide
//!
//! At each hide the launcher says how many characters its query had, never
//! the query. That is the proof that every key typed reached the field.

/// The line the launcher writes once per show when its query field gains
/// the keyboard.
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
