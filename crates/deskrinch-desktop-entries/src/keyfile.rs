//! The key-file syntax desktop entries are written in: groups of
//! `Key[locale]=value` lines, and the escapes of `string` and list values.
//!
//! This layer knows nothing about what the keys mean; [`crate::entry`] does.
//!
//! It reads files the way GLib's `GKeyFile` does. A file that breaks the
//! specification in a way GLib reads anyway is kept, and the deviation is
//! recorded. Only what GLib refuses outright fails the file: a line that is
//! not a comment, group or key, a malformed group or key name, or a key
//! before the first group.

use crate::deviation::Deviation;
use crate::error::ParseError;
use crate::locale::{Locale, Localized};

/// A parsed key file: its groups in file order, duplicates merged.
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
    locale: KeyLocale,
    raw: String,
}

#[derive(Debug, PartialEq, Eq)]
enum KeyLocale {
    None,
    Some(Locale),
    /// A locale this crate cannot match, such as `de_DE.UTF-8` or `[]`,
    /// which GLib accepts.
    Unusable(String),
}

impl KeyFile {
    /// Parses `bytes`, appending what deviates from the specification to
    /// `deviations`.
    pub(crate) fn parse(bytes: &[u8], deviations: &mut Vec<Deviation>) -> Result<Self, ParseError> {
        let mut groups: Vec<Group> = Vec::new();
        // The group the following keys belong to, as an index into `groups`.
        let mut current: Option<usize> = None;
        for (index, line) in bytes.split(|&b| b == b'\n').enumerate() {
            let number = index + 1;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = if let Ok(line) = std::str::from_utf8(line) {
                std::borrow::Cow::Borrowed(line)
            } else {
                deviations.push(Deviation::NotUtf8 { line: number });
                // A header still opens its group; a key line is dropped, its
                // value unreadable, as in GLib.
                if !line.trim_ascii_start().starts_with(b"[") {
                    continue;
                }
                String::from_utf8_lossy(line)
            };
            let line = line.trim_start();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                let name = header
                    .strip_suffix(']')
                    .filter(|name| valid_group_name(name))
                    .ok_or(ParseError::InvalidLine { line: number })?;
                current = Some(open_group(&mut groups, name, deviations));
                continue;
            }
            let key = parse_key_line(line, number)?;
            let group = &mut groups[current.ok_or(ParseError::KeyOutsideGroup { line: number })?];
            if !key
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            {
                deviations.push(Deviation::NonstandardKey {
                    group: group.name.clone(),
                    key: key.name.clone(),
                });
            }
            if let KeyLocale::Unusable(locale) = &key.locale {
                deviations.push(Deviation::UnusableLocale {
                    group: group.name.clone(),
                    key: format!("{}[{locale}]", key.name),
                });
            }
            // A repeated key replaces the earlier value, as in GLib.
            if let Some(earlier) = group
                .keys
                .iter()
                .position(|k| k.name == key.name && k.locale == key.locale)
            {
                deviations.push(Deviation::DuplicateKey {
                    group: group.name.clone(),
                    key: key.name.clone(),
                });
                group.keys.remove(earlier);
            }
            group.keys.push(key);
        }
        Ok(Self { groups })
    }

    /// The name of the first group, which must be `Desktop Entry`.
    pub(crate) fn first_group(&self) -> Option<&str> {
        self.groups.first().map(|group| group.name.as_str())
    }

    /// The group called `name`.
    pub(crate) fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }
}

impl Group {
    /// The group's name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The unlocalised value of `key`, unescaped as a `string`.
    pub(crate) fn string(&self, key: &str, deviations: &mut Vec<Deviation>) -> Option<String> {
        let (value, clean) = unescape_checked(self.raw(key)?);
        self.note_escapes(key, clean, deviations);
        Some(value)
    }

    /// The unlocalised value of `key` as a list of `string`s.
    pub(crate) fn list(&self, key: &str, deviations: &mut Vec<Deviation>) -> Vec<String> {
        let Some(raw) = self.raw(key) else {
            return Vec::new();
        };
        let (items, clean) = split_list_checked(raw);
        self.note_escapes(key, clean, deviations);
        items
    }

    /// The value of `key` as a `boolean`; absent is `false`.
    ///
    /// As GLib reads it: surrounding space is ignored, `1` and `0` mean
    /// `true` and `false` (a deviation), and any other value is a deviation
    /// read as `false`.
    pub(crate) fn boolean(&self, key: &str, deviations: &mut Vec<Deviation>) -> bool {
        let Some(raw) = self.raw(key) else {
            return false;
        };
        let (read_as, standard) = match raw.trim() {
            "true" => (true, true),
            "false" => (false, true),
            "1" => (true, false),
            _ => (false, false),
        };
        if !standard {
            deviations.push(Deviation::InvalidBoolean {
                group: self.name.clone(),
                key: key.into(),
                value: raw.into(),
                read_as,
            });
        }
        read_as
    }

    /// `key` with its translations, unescaped as `localestring`s. `None`
    /// when the key has no unlocalised value; translations of nothing are
    /// a deviation and are dropped.
    pub(crate) fn localized(
        &self,
        key: &str,
        deviations: &mut Vec<Deviation>,
    ) -> Option<Localized<String>> {
        self.localized_with(key, unescape_checked, deviations)
    }

    /// `key` with its translations, as lists of `localestring`s.
    pub(crate) fn localized_list(
        &self,
        key: &str,
        deviations: &mut Vec<Deviation>,
    ) -> Option<Localized<Vec<String>>> {
        self.localized_with(key, split_list_checked, deviations)
    }

    /// `key`'s translations alone, for a key whose unlocalised value is
    /// missing.
    pub(crate) fn translations(
        &self,
        key: &str,
        deviations: &mut Vec<Deviation>,
    ) -> Vec<(Locale, String)> {
        self.keys
            .iter()
            .filter(|k| k.name == key)
            .filter_map(|k| match &k.locale {
                KeyLocale::Some(locale) => {
                    let (value, clean) = unescape_checked(&k.raw);
                    self.note_escapes(key, clean, deviations);
                    Some((locale.clone(), value))
                }
                KeyLocale::None | KeyLocale::Unusable(_) => None,
            })
            .collect()
    }

    fn localized_with<T>(
        &self,
        key: &str,
        decode: fn(&str) -> (T, bool),
        deviations: &mut Vec<Deviation>,
    ) -> Option<Localized<T>> {
        let Some(raw) = self.raw(key) else {
            if self.keys.iter().any(|k| k.name == key) {
                deviations.push(Deviation::TranslationWithoutDefault {
                    group: self.name.clone(),
                    key: key.into(),
                });
            }
            return None;
        };
        let mut clean = true;
        let mut decode = |raw: &str| {
            let (value, ok) = decode(raw);
            clean &= ok;
            value
        };
        let mut value = Localized::new(decode(raw));
        value.translations = self
            .keys
            .iter()
            .filter(|k| k.name == key)
            .filter_map(|k| match &k.locale {
                KeyLocale::Some(locale) => Some((locale.clone(), decode(&k.raw))),
                KeyLocale::None | KeyLocale::Unusable(_) => None,
            })
            .collect();
        self.note_escapes(key, clean, deviations);
        Some(value)
    }

    fn note_escapes(&self, key: &str, clean: bool, deviations: &mut Vec<Deviation>) {
        if !clean {
            deviations.push(Deviation::InvalidEscape {
                group: self.name.clone(),
                key: key.into(),
            });
        }
    }

    fn raw(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|k| k.name == key && k.locale == KeyLocale::None)
            .map(|k| k.raw.as_str())
    }
}

/// The index of the group `name`, added if new. A group seen before is
/// reopened, so its keys merge, as in GLib.
fn open_group(groups: &mut Vec<Group>, name: &str, deviations: &mut Vec<Deviation>) -> usize {
    if let Some(existing) = groups.iter().position(|g| g.name == name) {
        deviations.push(Deviation::DuplicateGroup { group: name.into() });
        return existing;
    }
    groups.push(Group {
        name: name.into(),
        keys: Vec::new(),
    });
    groups.len() - 1
}

/// A group name, as GLib accepts it: not empty, without `[`, `]` or
/// control characters.
fn valid_group_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| !c.is_control() && c != '[' && c != ']')
}

/// Parses `Key[locale] = value`. Space around `=` is ignored.
///
/// The key name is checked as GLib checks it (`g_key_file_is_key_name`):
/// not empty, no `=`, `[` or `]`, no space at either end, and an optional
/// `[locale]` of letters, digits, `-`, `_`, `.` and `@`, ending the name. A
/// name GLib refuses fails the file, as it does there.
fn parse_key_line(line: &str, number: usize) -> Result<Key, ParseError> {
    let invalid = || ParseError::InvalidLine { line: number };
    let (left, value) = line.split_once('=').ok_or_else(invalid)?;
    let left = left.trim_end();
    let (name, locale) = match left.split_once('[') {
        Some((name, rest)) => {
            let locale = rest.strip_suffix(']').ok_or_else(invalid)?;
            let allowed = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@');
            if !locale.chars().all(allowed) {
                return Err(invalid());
            }
            let locale = match Locale::parse(locale) {
                // `Locale::parse` drops an encoding; a key's locale has
                // none to drop.
                Some(parsed) if !locale.contains('.') => KeyLocale::Some(parsed),
                _ => KeyLocale::Unusable(locale.into()),
            };
            (name, locale)
        }
        None => (left, KeyLocale::None),
    };
    let well_formed = !name.is_empty() && name.trim() == name && !name.contains(['[', ']', '=']);
    if !well_formed {
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
/// the `Exec` layer read them as intended, as KDE's reader does.
#[cfg(test)]
pub(crate) fn unescape(raw: &str) -> String {
    unescape_checked(raw).0
}

/// [`unescape`], and whether every escape was one the specification
/// defines.
fn unescape_checked(raw: &str) -> (String, bool) {
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
fn split_list_checked(raw: &str) -> (Vec<String>, bool) {
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

    fn parse(text: &str) -> (KeyFile, Vec<Deviation>) {
        let mut deviations = Vec::new();
        let file = KeyFile::parse(text.as_bytes(), &mut deviations).unwrap();
        (file, deviations)
    }

    #[test]
    fn reads_groups_keys_and_translations() {
        let (file, deviations) = parse(
            "# comment\n\n[Desktop Entry]\nName = Files\nName[de]=Dateien\r\nX-Thing=1\n[Other]\nName=x\n",
        );
        assert_eq!(deviations, []);
        assert_eq!(file.first_group(), Some("Desktop Entry"));
        let group = file.group("Desktop Entry").unwrap();
        let mut d = Vec::new();
        let name = group.localized("Name", &mut d).unwrap();
        assert_eq!(name.default, "Files");
        assert_eq!(name.get(&[Locale::parse("de_DE").unwrap()]), "Dateien");
        assert_eq!(group.string("X-Thing", &mut d).as_deref(), Some("1"));
        let other = file.group("Other").unwrap();
        assert_eq!(other.string("Name", &mut d).as_deref(), Some("x"));
        assert_eq!(d, []);
    }

    #[test]
    fn fails_only_what_glib_refuses() {
        for (text, expected) in [
            ("Name=x\n", ParseError::KeyOutsideGroup { line: 1 }),
            ("[A]\nno equals\n", ParseError::InvalidLine { line: 2 }),
            ("[A]\n=x\n", ParseError::InvalidLine { line: 2 }),
            ("[A]\nName [de]=x\n", ParseError::InvalidLine { line: 2 }),
            ("[A]\nName[d e]=x\n", ParseError::InvalidLine { line: 2 }),
            ("[A]\nName[de]x=1\n", ParseError::InvalidLine { line: 2 }),
            ("[A\n", ParseError::InvalidLine { line: 1 }),
            ("[Bad]Group]\n", ParseError::InvalidLine { line: 1 }),
            ("[Gr\u{1}oup]\n", ParseError::InvalidLine { line: 1 }),
        ] {
            let error = KeyFile::parse(text.as_bytes(), &mut Vec::new()).unwrap_err();
            assert_eq!(error, expected, "{text:?}");
        }
    }

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
