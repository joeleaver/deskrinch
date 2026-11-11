//! Bounding and cleaning the plain strings an untrusted peer hands over.

/// Whether line breaks survive in a cleaned string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    /// `\n` and `\t` are kept; other control characters are dropped.
    Keep,
    /// `\n` and `\t` become spaces; other control characters are dropped.
    Join,
}

/// `input` with control characters removed (see [`Lines`]), truncated at a
/// character boundary to at most `max_bytes` bytes.
#[must_use]
pub fn clean(input: &str, max_bytes: usize, lines: Lines) -> String {
    let mut out = String::with_capacity(input.len().min(max_bytes));
    push_clean(&mut out, input, max_bytes, lines);
    out
}

/// Appends `input`, cleaned as [`clean`] does, to `out` while `out` stays
/// within `max_bytes`. Returns `false` once `input` did not fit entirely.
pub fn push_clean(out: &mut String, input: &str, max_bytes: usize, lines: Lines) -> bool {
    for c in input.chars() {
        let Some(c) = cleaned_char(c, lines) else {
            continue;
        };
        if out.len() + c.len_utf8() > max_bytes {
            return false;
        }
        out.push(c);
    }
    true
}

/// `c` as it appears in a cleaned string, or `None` when it is dropped.
fn cleaned_char(c: char, lines: Lines) -> Option<char> {
    match c {
        '\n' | '\t' => Some(match lines {
            Lines::Keep => c,
            Lines::Join => ' ',
        }),
        // A carriage return is a line break from a sender that writes CRLF.
        '\r' => None,
        c if c.is_control() => None,
        c => Some(c),
    }
}

/// A non-empty, cleaned, single-line string, or `None` when nothing is left.
#[must_use]
pub fn non_empty(input: &str, max_bytes: usize) -> Option<String> {
    let s = clean(input.trim(), max_bytes, Lines::Join);
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(
            clean("a\u{7}b\u{1b}[31mc\u{9b}d", 64, Lines::Keep),
            "ab[31mcd"
        );
    }

    #[test]
    fn line_breaks_are_kept_or_joined() {
        assert_eq!(clean("a\r\nb\tc", 64, Lines::Keep), "a\nb\tc");
        assert_eq!(clean("a\r\nb\tc", 64, Lines::Join), "a b c");
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        // "é" is two bytes; three bytes hold "aé" but not "aéé".
        assert_eq!(clean("aéé", 3, Lines::Keep), "aé");
        assert_eq!(clean("aéé", 4, Lines::Keep), "aé");
        assert_eq!(clean("aéé", 5, Lines::Keep), "aéé");
        assert_eq!(clean("abc", 0, Lines::Keep), "");
    }

    #[test]
    fn non_empty_trims_and_rejects_blank() {
        assert_eq!(non_empty("  firefox \n", 64).as_deref(), Some("firefox"));
        assert_eq!(non_empty(" \t ", 64), None);
        assert_eq!(non_empty("\u{7}", 64), None);
    }
}
