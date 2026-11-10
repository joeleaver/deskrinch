//! The key-file syntax desktop entries are written in: groups of
//! `Key[locale]=value` lines, and the escapes of `string` and list values.
//!
//! This layer knows nothing about what the keys mean; [`crate::entry`] does.

use crate::error::ParseError;
use crate::locale::{Locale, Localized};

/// A parsed key file: its groups in file order.
#[derive(Debug)]
pub(crate) struct KeyFile {
    groups: Vec<Group>,
}

/// One `[Group Name]` and its keys.
#[derive(Debug)]
pub(crate) struct Group {
    name: String,
    keys: Vec<Key>,
}

/// One `Key[locale]=value` line, with the value still escaped.
#[derive(Debug)]
struct Key {
    name: String,
    locale: Option<Locale>,
    raw: String,
}

impl KeyFile {
    /// Parses `text`. A malformed line, a key before the first group, a
    /// repeated group or a repeated key fails the whole file, as the
    /// specification makes such a file invalid.
    pub(crate) fn parse(text: &str) -> Result<Self, ParseError> {
        let mut groups: Vec<Group> = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                let name = header
                    .strip_suffix(']')
                    .filter(|name| valid_group_name(name))
                    .ok_or(ParseError::InvalidLine { line: number })?;
                if groups.iter().any(|group| group.name == name) {
                    return Err(ParseError::DuplicateGroup { group: name.into() });
                }
                groups.push(Group {
                    name: name.into(),
                    keys: Vec::new(),
                });
                continue;
            }
            let key = parse_key_line(line, number)?;
            let group = groups
                .last_mut()
                .ok_or(ParseError::KeyOutsideGroup { line: number })?;
            if group
                .keys
                .iter()
                .any(|other| other.name == key.name && other.locale == key.locale)
            {
                return Err(ParseError::DuplicateKey {
                    group: group.name.clone(),
                    key: key.name,
                });
            }
            group.keys.push(key);
        }
        Ok(Self { groups })
    }

    /// The group called `name`.
    pub(crate) fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }
}

impl Group {
    /// The unlocalised value of `key`, unescaped as a `string`.
    pub(crate) fn string(&self, key: &str) -> Option<String> {
        self.raw(key).map(unescape)
    }

    /// The unlocalised value of `key` as a list of `string`s.
    pub(crate) fn list(&self, key: &str) -> Vec<String> {
        self.raw(key).map(split_list).unwrap_or_default()
    }

    /// The value of `key` as a `boolean`; absent is `false`.
    pub(crate) fn boolean(&self, key: &str) -> Result<bool, ParseError> {
        match self.raw(key) {
            None | Some("false") => Ok(false),
            Some("true") => Ok(true),
            Some(value) => Err(ParseError::InvalidBoolean {
                key: key.into(),
                value: value.into(),
            }),
        }
    }

    /// `key` with its translations, unescaped as `localestring`s. `None`
    /// when the key has no unlocalised value: a translation of nothing is
    /// not a value.
    pub(crate) fn localized(&self, key: &str) -> Option<Localized<String>> {
        self.localized_with(key, unescape)
    }

    /// `key` with its translations, as lists of `localestring`s.
    pub(crate) fn localized_list(&self, key: &str) -> Option<Localized<Vec<String>>> {
        self.localized_with(key, split_list)
    }

    fn localized_with<T>(&self, key: &str, decode: fn(&str) -> T) -> Option<Localized<T>> {
        let mut value = Localized::new(decode(self.raw(key)?));
        value.translations = self
            .keys
            .iter()
            .filter(|k| k.name == key)
            .filter_map(|k| Some((k.locale.clone()?, decode(&k.raw))))
            .collect();
        Some(value)
    }

    fn raw(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|k| k.name == key && k.locale.is_none())
            .map(|k| k.raw.as_str())
    }
}

/// A group name is printable ASCII without `[` or `]`.
fn valid_group_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control() && c != '[' && c != ']')
}

/// Parses `Key[locale] = value`. Space around `=` is ignored.
fn parse_key_line(line: &str, number: usize) -> Result<Key, ParseError> {
    let invalid = || ParseError::InvalidLine { line: number };
    let (left, value) = line.split_once('=').ok_or_else(invalid)?;
    let left = left.trim_end();
    let (name, locale) = match left.split_once('[') {
        Some((name, locale)) => {
            let locale = locale.strip_suffix(']').ok_or_else(invalid)?;
            // A key's locale has no encoding; `Locale::parse` would drop one
            // silently, so reject it here.
            if locale.contains('.') {
                return Err(invalid());
            }
            (name, Some(Locale::parse(locale).ok_or_else(invalid)?))
        }
        None => (left, None),
    };
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(invalid());
    }
    Ok(Key {
        name: name.into(),
        locale,
        raw: value.trim_start().into(),
    })
}

/// Undoes the `string` escapes `\s`, `\n`, `\t`, `\r` and `\\`.
///
/// Any other backslash is kept with the character after it. The
/// specification defines no other escapes, but real files write the `Exec`
/// quoting escapes (`\$`, `\"`) with a single backslash; keeping them lets
/// the `Exec` layer read them as intended, as GLib's and KDE's readers do.
pub(crate) fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
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
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Splits a list value at unescaped `;`, undoes `\;`, then unescapes each
/// element as a `string`. Empty elements, including the one after the
/// terminating `;`, carry nothing and are dropped.
pub(crate) fn split_list(raw: &str) -> Vec<String> {
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
    items
        .iter()
        .map(|item| unescape(item))
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_groups_keys_and_translations() {
        let file = KeyFile::parse(
            "# comment\n\n[Desktop Entry]\nName = Files\nName[de]=Dateien\r\nX-Thing=1\n[Other]\nName=x\n",
        )
        .unwrap();
        let group = file.group("Desktop Entry").unwrap();
        let name = group.localized("Name").unwrap();
        assert_eq!(name.default, "Files");
        assert_eq!(name.get(&[Locale::parse("de_DE").unwrap()]), "Dateien");
        assert_eq!(group.string("X-Thing").as_deref(), Some("1"));
        assert_eq!(
            file.group("Other").unwrap().string("Name").as_deref(),
            Some("x")
        );
    }

    #[test]
    fn rejects_invalid_structure() {
        for (text, expected) in [
            ("Name=x\n", ParseError::KeyOutsideGroup { line: 1 }),
            (
                "[A]\n[A]\n",
                ParseError::DuplicateGroup { group: "A".into() },
            ),
            (
                "[A]\nK=1\nK=2\n",
                ParseError::DuplicateKey {
                    group: "A".into(),
                    key: "K".into(),
                },
            ),
            ("[A]\nno equals\n", ParseError::InvalidLine { line: 2 }),
            ("[A]\nK_y=1\n", ParseError::InvalidLine { line: 2 }),
            ("[A]\nK[de.UTF-8]=1\n", ParseError::InvalidLine { line: 2 }),
            ("[A\n", ParseError::InvalidLine { line: 1 }),
        ] {
            assert_eq!(KeyFile::parse(text).unwrap_err(), expected, "{text:?}");
        }
    }

    #[test]
    fn a_translation_without_a_default_is_no_value() {
        let file = KeyFile::parse("[A]\nName[de]=x\n").unwrap();
        assert!(file.group("A").unwrap().localized("Name").is_none());
    }

    #[test]
    fn unescapes_strings() {
        assert_eq!(unescape(r"a\sb\tc\nd\re\\f"), "a b\tc\nd\re\\f");
        assert_eq!(unescape(r#"\$x \"y\" \"#), r#"\$x \"y\" \"#);
    }

    #[test]
    fn splits_lists() {
        assert_eq!(split_list("a;b;"), ["a", "b"]);
        assert_eq!(split_list("a;b"), ["a", "b"]);
        assert_eq!(split_list(r"a\;b;c\sd;;"), ["a;b", "c d"]);
        assert_eq!(split_list(""), Vec::<String>::new());
    }

    #[test]
    fn booleans_are_true_or_false() {
        let file = KeyFile::parse("[A]\nT=true\nF=false\nX=1\n").unwrap();
        let group = file.group("A").unwrap();
        assert_eq!(group.boolean("T"), Ok(true));
        assert_eq!(group.boolean("F"), Ok(false));
        assert_eq!(group.boolean("Absent"), Ok(false));
        assert!(group.boolean("X").is_err());
    }
}
