//! The escapes of `string` and list values.

/// Undoes the `string` escapes `\s`, `\n`, `\t`, `\r` and `\\`.
///
/// Any other backslash is kept with the character after it. The
/// specification defines no other escapes, but real desktop entries write
/// the `Exec` quoting escapes (`\$`, `\"`) with a single backslash; keeping
/// them lets the `Exec` layer read them as intended, as KDE's reader does.
#[must_use]
pub fn unescape(raw: &str) -> String {
    unescape_checked(raw).0
}

/// [`unescape`], and whether every escape was one the specification
/// defines.
pub(crate) fn unescape_checked(raw: &str) -> (String, bool) {
    let mut out = String::with_capacity(raw.len());
    let mut clean = true;
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            None => {
                clean = false;
                out.push('\\');
            }
            Some(other) => {
                clean = false;
                out.push('\\');
                out.push(other);
            }
        }
    }
    (out, clean)
}

/// Splits a list value at unescaped `;`, undoes `\;`, then unescapes each
/// element as a `string`. Empty elements, including the one after the
/// terminating `;`, carry nothing and are dropped.
pub(crate) fn split_list_checked(raw: &str) -> (Vec<String>, bool) {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            ';' => items.push(std::mem::take(&mut current)),
            '\\' => match chars.next() {
                Some(';') => current.push(';'),
                Some(other) => {
                    current.push('\\');
                    current.push(other);
                }
                None => current.push('\\'),
            },
            c => current.push(c),
        }
    }
    items.push(current);
    let mut clean = true;
    let items = items
        .iter()
        .map(|item| {
            let (item, ok) = unescape_checked(item);
            clean &= ok;
            item
        })
        .filter(|item| !item.is_empty())
        .collect();
    (items, clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_strings() {
        assert_eq!(unescape(r"a\sb\tc\nd\re\\f"), "a b\tc\nd\re\\f");
        assert_eq!(unescape_checked(r"a\sb\\"), ("a b\\".into(), true));
        assert_eq!(
            unescape_checked(r#"\$x \"y\""#),
            (r#"\$x \"y\""#.into(), false)
        );
        assert_eq!(unescape_checked("x\\"), ("x\\".into(), false));
    }

    #[test]
    fn splits_lists() {
        let items = |raw| split_list_checked(raw).0;
        assert_eq!(items("a;b;"), ["a", "b"]);
        assert_eq!(items("a;b"), ["a", "b"]);
        assert_eq!(items(r"a\;b;c\sd;;"), ["a;b", "c d"]);
        assert_eq!(items(""), Vec::<String>::new());
        assert!(!split_list_checked(r"a\$;").1);
    }
}
