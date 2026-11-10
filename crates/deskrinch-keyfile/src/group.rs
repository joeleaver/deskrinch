//! Reading a group's values as the specification's types: `string`,
//! `localestring`, `boolean`, `numeric` integers and lists.

use crate::error::{Deviation, Report};
use crate::escape::{split_list_checked, split_list_with, unescape_checked};
use crate::file::{Group, KeyLocale};
use crate::locale::{Locale, Localized};

impl Group {
    /// The group's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the group has `key`, with or without a locale.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.keys.iter().any(|k| k.name == key)
    }

    /// The unlocalised value of `key`, unescaped as a `string`.
    pub fn string(&self, key: &str, deviations: &mut impl Report) -> Option<String> {
        let (value, clean) = unescape_checked(self.raw(key)?);
        self.note_escapes(key, clean, deviations);
        Some(value)
    }

    /// The unlocalised value of `key` as a list of `string`s: split at
    /// unescaped `;`, empty elements dropped. Absent is empty.
    pub fn list(&self, key: &str, deviations: &mut impl Report) -> Vec<String> {
        self.list_separated(key, ';', deviations)
    }

    /// [`list`](Self::list) with another separator: icon themes separate
    /// `Directories` and `Inherits` with `,` (GTK sets the key file's list
    /// separator to `,` for them).
    pub fn list_separated(
        &self,
        key: &str,
        separator: char,
        deviations: &mut impl Report,
    ) -> Vec<String> {
        let Some(raw) = self.raw(key) else {
            return Vec::new();
        };
        let (items, clean) = split_list_with(raw, separator);
        self.note_escapes(key, clean, deviations);
        items
    }

    /// The value of `key` as a `boolean`; absent is `false`.
    ///
    /// As GLib reads it: surrounding space is ignored, `1` and `0` mean
    /// `true` and `false` (a deviation), and any other value is a deviation
    /// read as `false`.
    pub fn boolean(&self, key: &str, deviations: &mut impl Report) -> bool {
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
            deviations.report(Deviation::InvalidBoolean {
                group: self.name.clone(),
                key: key.into(),
                value: raw.into(),
                read_as,
            });
        }
        read_as
    }

    /// The value of `key` as an integer, such as an icon directory's
    /// `Size`. As GLib reads it: surrounding space is ignored and the rest
    /// must be a whole decimal number, optionally signed, that fits in an
    /// `i32`. Anything else is a deviation read as absent.
    pub fn integer(&self, key: &str, deviations: &mut impl Report) -> Option<i32> {
        let raw = self.raw(key)?;
        let parsed = raw.trim().parse::<i32>().ok();
        if parsed.is_none() {
            deviations.report(Deviation::InvalidInteger {
                group: self.name.clone(),
                key: key.into(),
                value: raw.into(),
            });
        }
        parsed
    }

    /// `key` with its translations, unescaped as `localestring`s. `None`
    /// when the key has no unlocalised value; translations of nothing are
    /// a deviation and are dropped.
    pub fn localized(&self, key: &str, deviations: &mut impl Report) -> Option<Localized<String>> {
        self.localized_with(key, unescape_checked, deviations)
    }

    /// `key` with its translations, as lists of `localestring`s.
    pub fn localized_list(
        &self,
        key: &str,
        deviations: &mut impl Report,
    ) -> Option<Localized<Vec<String>>> {
        self.localized_with(key, split_list_checked, deviations)
    }

    /// `key`'s translations alone, for a key whose unlocalised value is
    /// missing.
    pub fn translations(&self, key: &str, deviations: &mut impl Report) -> Vec<(Locale, String)> {
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
        deviations: &mut impl Report,
    ) -> Option<Localized<T>> {
        let Some(raw) = self.raw(key) else {
            if self.has(key) {
                deviations.report(Deviation::TranslationWithoutDefault {
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

    fn note_escapes(&self, key: &str, clean: bool, deviations: &mut impl Report) {
        if !clean {
            deviations.report(Deviation::InvalidEscape {
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

#[cfg(test)]
mod tests {
    use crate::{Deviation, KeyFile, Locale};

    fn group(text: &str) -> crate::Group {
        let file = KeyFile::parse(text.as_bytes(), &mut Vec::<Deviation>::new()).unwrap();
        file.groups().next().unwrap().clone()
    }

    #[test]
    fn reads_strings_lists_and_translations() {
        let g = group("[A]\nName = Files\nName[de]=Dateien\nList=a;b\\;c;\nX-Thing=1\n");
        let mut d = Vec::<Deviation>::new();
        let name = g.localized("Name", &mut d).unwrap();
        assert_eq!(name.default, "Files");
        assert_eq!(name.get(&[Locale::parse("de_DE").unwrap()]), "Dateien");
        assert_eq!(g.list("List", &mut d), ["a", "b;c"]);
        assert_eq!(g.string("X-Thing", &mut d).as_deref(), Some("1"));
        assert!(g.has("Name") && !g.has("Missing"));
        assert_eq!(d, []);
    }

    #[test]
    fn reads_integers_as_glib_does() {
        let g = group("[A]\nA=48\nB= -2 \nC=+3\nD=4x\nE=\nF=99999999999\n");
        let mut d = Vec::<Deviation>::new();
        assert_eq!(g.integer("A", &mut d), Some(48));
        assert_eq!(g.integer("B", &mut d), Some(-2));
        assert_eq!(g.integer("C", &mut d), Some(3));
        assert_eq!(g.integer("Absent", &mut d), None);
        assert_eq!(d, []);
        for key in ["D", "E", "F"] {
            assert_eq!(g.integer(key, &mut d), None, "{key}");
        }
        assert_eq!(d.len(), 3);
        assert!(matches!(&d[0], Deviation::InvalidInteger { key, .. } if key == "D"));
    }
}
