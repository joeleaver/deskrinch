//! The settings file a stand-in process answers from: one setting a line,
//! its namespace, key and value separated by spaces, the value a number or
//! a string in double quotes. Blank lines and lines starting with `#` are
//! skipped.
//!
//! ```text
//! org.freedesktop.appearance color-scheme 2
//! org.kde.kdeglobals.Icons Theme "breeze"
//! ```

use std::fmt::Write as _;

use crate::{Setting, Value};

/// A line of a settings file that is not a setting, or a value that cannot
/// be written as one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {problem}")]
pub struct FileError {
    /// The line, counted from 1.
    pub line: usize,
    /// What is wrong with it.
    pub problem: &'static str,
}

/// The settings in `text`, in its order; a setting given twice is answered
/// with its last value.
///
/// # Errors
///
/// The first line that is not a setting.
pub fn parse(text: &str) -> Result<Vec<(Setting<'_>, Value)>, FileError> {
    let mut settings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let error = |problem| FileError {
            line: index + 1,
            problem,
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let (Some(namespace), Some(key), Some(value)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(error("not a namespace, a key and a value"));
        };
        let value = value.trim();
        let value = if let Some(quoted) = value.strip_prefix('"') {
            match quoted.strip_suffix('"') {
                Some(text) if !text.contains('"') => Value::text(text),
                _ => return Err(error("a string that is not one quoted string")),
            }
        } else {
            Value::Number(
                value
                    .parse()
                    .map_err(|_| error("a value that is neither a u32 nor a quoted string"))?,
            )
        };
        settings.push((Setting { namespace, key }, value));
    }
    Ok(settings)
}

/// The text of a settings file saying `settings`, which [`parse`] reads
/// back.
///
/// # Errors
///
/// When a namespace or key is empty or has a space, or a string has a
/// double quote or a line break: none can be written. The line is the
/// setting's, counted from 1.
pub fn format<'a>(
    settings: impl IntoIterator<Item = (Setting<'a>, &'a Value)>,
) -> Result<String, FileError> {
    let mut text = String::new();
    for (index, (setting, value)) in settings.into_iter().enumerate() {
        let error = |problem| FileError {
            line: index + 1,
            problem,
        };
        let name_ok = |name: &str| !name.is_empty() && !name.contains(char::is_whitespace);
        if !name_ok(setting.namespace) || !name_ok(setting.key) {
            return Err(error("a namespace or key that is empty or has a space"));
        }
        let Setting { namespace, key } = setting;
        // Writing to a `String` cannot fail.
        let _written = match value {
            Value::Number(number) => writeln!(text, "{namespace} {key} {number}"),
            Value::Text(string) if string.contains(['"', '\n', '\r']) => {
                return Err(error("a string with a double quote or a line break"));
            }
            Value::Text(string) => writeln!(text, "{namespace} {key} \"{string}\""),
        };
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{COLOR_SCHEME, KDE_ICON_THEME};

    #[test]
    fn reads_numbers_strings_and_skips_comments() {
        let text = "# the scheme\norg.freedesktop.appearance color-scheme 2\n\n\
                    org.kde.kdeglobals.Icons Theme \"Papirus Dark\"\n";
        assert_eq!(
            parse(text).unwrap(),
            [
                (COLOR_SCHEME, Value::Number(2)),
                (KDE_ICON_THEME, Value::text("Papirus Dark")),
            ]
        );
    }

    #[test]
    fn reports_the_line_that_is_not_a_setting() {
        for (text, line) in [
            ("a b\n", 1),
            ("# fine\na b dark\n", 2),
            ("a b \"open\n", 1),
            ("a b 1\na b \"x\"y\"\n", 2),
        ] {
            assert_eq!(parse(text).unwrap_err().line, line, "{text:?}");
        }
    }

    #[test]
    fn writes_what_it_reads() {
        let settings = [
            (COLOR_SCHEME, Value::Number(1)),
            (KDE_ICON_THEME, Value::text("breeze-dark")),
        ];
        let text = format(settings.iter().map(|(s, v)| (*s, v))).unwrap();
        assert_eq!(parse(&text).unwrap(), settings);
    }

    #[test]
    fn refuses_what_it_cannot_write() {
        let quote = Value::text("a\"b");
        assert_eq!(format([(COLOR_SCHEME, &quote)]).unwrap_err().line, 1);
        let spaced = Setting {
            namespace: "a b",
            key: "c",
        };
        assert!(format([(spaced, &Value::Number(1))]).is_err());
    }
}
