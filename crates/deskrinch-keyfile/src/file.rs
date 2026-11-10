//! Reading a key file into groups of `Key[locale]=value` lines.

use crate::error::{Deviation, Report, SyntaxError};
use crate::locale::Locale;

/// A parsed key file: its groups in file order, duplicates merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFile {
    groups: Vec<Group>,
}

/// One `[Group Name]` and its keys, values still escaped until an accessor
/// such as [`string`](Self::string) or [`integer`](Self::integer) reads
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub(crate) name: String,
    pub(crate) keys: Vec<Key>,
}

/// One `Key[locale]=value` line, with the value still escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) name: String,
    pub(crate) locale: KeyLocale,
    pub(crate) raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyLocale {
    None,
    Some(Locale),
    /// A locale that cannot be matched, such as `de_DE.UTF-8` or `[]`,
    /// which GLib accepts.
    Unusable(String),
}

impl KeyFile {
    /// Parses `bytes`, reporting what deviates from the specification.
    ///
    /// # Errors
    ///
    /// [`SyntaxError`] for what GLib refuses outright: a line that is not
    /// a comment, group or key, a malformed group or key name, or a key
    /// before the first group.
    pub fn parse(bytes: &[u8], deviations: &mut impl Report) -> Result<Self, SyntaxError> {
        let mut groups: Vec<Group> = Vec::new();
        // The group the following keys belong to, as an index into `groups`.
        let mut current: Option<usize> = None;
        for (index, line) in bytes.split(|&b| b == b'\n').enumerate() {
            let number = index + 1;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = if let Ok(line) = std::str::from_utf8(line) {
                std::borrow::Cow::Borrowed(line)
            } else {
                deviations.report(Deviation::NotUtf8 { line: number });
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
                    .ok_or(SyntaxError::InvalidLine { line: number })?;
                current = Some(open_group(&mut groups, name, deviations));
                continue;
            }
            let key = parse_key_line(line, number)?;
            let group =
                &mut groups[current.ok_or(SyntaxError::KeyOutsideGroup { line: number })?];
            group.add(key, deviations);
        }
        Ok(Self { groups })
    }

    /// The name of the first group, which a format may require to be its
    /// own (`Desktop Entry`, `Icon Theme`).
    #[must_use]
    pub fn first_group(&self) -> Option<&str> {
        self.groups.first().map(|group| group.name.as_str())
    }

    /// The group called `name`.
    #[must_use]
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.name == name)
    }

    /// Every group, in file order.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter()
    }
}

impl Group {
    /// Adds a key line, reporting its oddities. A repeated key replaces the
    /// earlier value, as in GLib.
    fn add(&mut self, key: Key, deviations: &mut impl Report) {
        if !key
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            deviations.report(Deviation::NonstandardKey {
                group: self.name.clone(),
                key: key.name.clone(),
            });
        }
        if let KeyLocale::Unusable(locale) = &key.locale {
            deviations.report(Deviation::UnusableLocale {
                group: self.name.clone(),
                key: format!("{}[{locale}]", key.name),
            });
        }
        if let Some(earlier) = self
            .keys
            .iter()
            .position(|k| k.name == key.name && k.locale == key.locale)
        {
            deviations.report(Deviation::DuplicateKey {
                group: self.name.clone(),
                key: key.name.clone(),
            });
            self.keys.remove(earlier);
        }
        self.keys.push(key);
    }
}

/// The index of the group `name`, added if new. A group seen before is
/// reopened, so its keys merge, as in GLib.
fn open_group(groups: &mut Vec<Group>, name: &str, deviations: &mut impl Report) -> usize {
    if let Some(existing) = groups.iter().position(|g| g.name == name) {
        deviations.report(Deviation::DuplicateGroup { group: name.into() });
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
fn parse_key_line(line: &str, number: usize) -> Result<Key, SyntaxError> {
    let invalid = || SyntaxError::InvalidLine { line: number };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (KeyFile, Vec<Deviation>) {
        let mut deviations = Vec::new();
        let file = KeyFile::parse(text.as_bytes(), &mut deviations).unwrap();
        (file, deviations)
    }

    #[test]
    fn reads_groups_in_order_and_merges_repeats() {
        let (file, deviations) = parse(
            "# comment\n\n[Desktop Entry]\nName = Files\r\n[Other]\nA=1\n[Desktop Entry]\nName=x\n",
        );
        assert_eq!(file.first_group(), Some("Desktop Entry"));
        let names: Vec<&str> = file.groups().map(Group::name).collect();
        assert_eq!(names, ["Desktop Entry", "Other"]);
        assert_eq!(
            deviations,
            [
                Deviation::DuplicateGroup {
                    group: "Desktop Entry".into()
                },
                Deviation::DuplicateKey {
                    group: "Desktop Entry".into(),
                    key: "Name".into()
                }
            ]
        );
    }

    #[test]
    fn fails_only_what_glib_refuses() {
        for (text, expected) in [
            ("Name=x\n", SyntaxError::KeyOutsideGroup { line: 1 }),
            ("[A]\nno equals\n", SyntaxError::InvalidLine { line: 2 }),
            ("[A]\n=x\n", SyntaxError::InvalidLine { line: 2 }),
            ("[A]\nName [de]=x\n", SyntaxError::InvalidLine { line: 2 }),
            ("[A]\nName[d e]=x\n", SyntaxError::InvalidLine { line: 2 }),
            ("[A]\nName[de]x=1\n", SyntaxError::InvalidLine { line: 2 }),
            ("[A\n", SyntaxError::InvalidLine { line: 1 }),
            ("[Bad]Group]\n", SyntaxError::InvalidLine { line: 1 }),
            ("[Gr\u{1}oup]\n", SyntaxError::InvalidLine { line: 1 }),
        ] {
            let error = KeyFile::parse(text.as_bytes(), &mut Vec::<Deviation>::new()).unwrap_err();
            assert_eq!(error, expected, "{text:?}");
        }
    }
}
